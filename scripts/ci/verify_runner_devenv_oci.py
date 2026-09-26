#!/usr/bin/env python3
"""Verify the OCI archive produced by the credentialless RunnerDevEnv proof."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import re
import tarfile
from pathlib import Path
from typing import Any


DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
IMAGE_CONFIG_TYPES = {
    "application/vnd.oci.image.config.v1+json",
    "application/vnd.docker.container.image.v1+json",
}
IMAGE_MANIFEST_TYPES = {
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
}


class VerificationError(ValueError):
    pass


def digest_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def require_digest(value: Any, where: str) -> str:
    if not isinstance(value, str) or not DIGEST_RE.fullmatch(value):
        raise VerificationError(f"{where} is not a valid sha256 digest")
    return value


def blob_path(digest: str) -> str:
    algorithm, _, value = digest.partition(":")
    require_digest(digest, "OCI descriptor digest")
    return f"blobs/{algorithm}/{value}"


def descriptor_parts(descriptor: Any, where: str) -> tuple[str, int]:
    if not isinstance(descriptor, dict):
        raise VerificationError(f"{where} is not an OCI descriptor")
    digest = require_digest(descriptor.get("digest"), f"{where} digest")
    size = descriptor.get("size")
    if not isinstance(size, int) or isinstance(size, bool) or size < 0:
        raise VerificationError(f"{where} has no valid byte size")
    return digest, size


def verify_member(
    archive: tarfile.TarFile, descriptor: Any, where: str, *, capture: bool = False
) -> bytes | None:
    digest, expected_size = descriptor_parts(descriptor, where)
    stream = archive.extractfile(blob_path(digest))
    if stream is None:
        raise VerificationError(f"OCI blob is missing: {digest}")
    hasher = hashlib.sha256()
    chunks = [] if capture else None
    actual_size = 0
    while chunk := stream.read(1024 * 1024):
        actual_size += len(chunk)
        hasher.update(chunk)
        if chunks is not None:
            chunks.append(chunk)
    if actual_size != expected_size:
        raise VerificationError(f"OCI blob size mismatch: {digest}")
    if "sha256:" + hasher.hexdigest() != digest:
        raise VerificationError(f"OCI blob digest mismatch: {digest}")
    return b"".join(chunks) if chunks is not None else None


def read_json_member(archive: tarfile.TarFile, name: str) -> tuple[bytes, Any]:
    stream = archive.extractfile(name)
    if stream is None:
        raise VerificationError(f"OCI archive member is missing: {name}")
    raw = stream.read()
    try:
        return raw, json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"OCI archive member is not valid JSON: {name}") from error


def verify_attestation_payload(
    archive: tarfile.TarFile,
    descriptor: dict[str, Any],
    manifest: dict[str, Any],
    image_manifest_digest: str,
) -> bool:
    annotations = descriptor.get("annotations", {})
    if annotations.get("vnd.docker.reference.type") != "attestation-manifest":
        return False
    if annotations.get("vnd.docker.reference.digest") != image_manifest_digest:
        return False

    for layer in manifest.get("layers", []):
        payload = verify_member(archive, layer, "attestation layer", capture=True)
        assert payload is not None
        if layer.get("mediaType", "").endswith("+gzip"):
            try:
                payload = gzip.decompress(payload)
            except OSError as error:
                raise VerificationError("SLSA provenance layer is not valid gzip") from error
        try:
            statement = json.loads(payload)
        except (UnicodeDecodeError, json.JSONDecodeError):
            continue
        if not isinstance(statement, dict):
            continue
        predicate = statement.get("predicateType", "")
        if not predicate.startswith("https://slsa.dev/provenance/"):
            continue
        subjects = statement.get("subject", [])
        if any(
            isinstance(subject, dict)
            and isinstance(subject.get("digest"), dict)
            and subject.get("digest", {}).get("sha256") == image_manifest_digest.removeprefix("sha256:")
            for subject in subjects
        ):
            return True
    return False


def verify_archive(
    archive_path: Path,
    metadata_path: Path,
    image_ref: str,
    source_sha: str,
) -> dict[str, str]:
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise VerificationError("dispatch source SHA is malformed")
    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerificationError("BuildKit metadata file is unreadable or invalid") from error

    try:
        with tarfile.open(archive_path, "r:*") as archive:
            _, layout = read_json_member(archive, "oci-layout")
            if layout.get("imageLayoutVersion") != "1.0.0":
                raise VerificationError("unsupported OCI image layout version")
            index_raw, index = read_json_member(archive, "index.json")
            if index.get("schemaVersion") != 2:
                raise VerificationError("OCI index has an unsupported schema version")
            index_digest = digest_bytes(index_raw)
            root_descriptors = index.get("manifests", [])
            if not isinstance(root_descriptors, list) or not root_descriptors:
                raise VerificationError("OCI index contains no root descriptors")
            expected_ref_names = {image_ref, image_ref.rsplit(":", 1)[-1]}
            named_roots = [
                descriptor for descriptor in root_descriptors
                if expected_ref_names.intersection(
                    name.strip()
                    for name in descriptor.get("annotations", {}).get(
                        "org.opencontainers.image.ref.name", ""
                    ).split(",")
                )
            ]
            if len(named_roots) != 1:
                observed = sorted({
                    descriptor.get("annotations", {}).get("org.opencontainers.image.ref.name", "")
                    for descriptor in root_descriptors
                })
                raise VerificationError(
                    f"OCI index must name the requested image or its exact tag; observed refs: {observed}"
                )

            documents: dict[str, dict[str, Any]] = {}
            descriptors: dict[str, dict[str, Any]] = {}
            pending = list(root_descriptors)
            while pending:
                descriptor = pending.pop()
                digest, _ = descriptor_parts(descriptor, "OCI graph descriptor")
                previous = descriptors.get(digest)
                if previous is not None:
                    if previous.get("size") != descriptor.get("size"):
                        raise VerificationError(f"OCI graph has conflicting sizes for {digest}")
                    continue
                descriptors[digest] = descriptor
                raw = verify_member(archive, descriptor, "OCI graph descriptor", capture=True)
                assert raw is not None
                media_type = descriptor.get("mediaType", "")
                if media_type.endswith(".manifest.v1+json") or media_type.endswith(".manifest.v2+json") or media_type.endswith(".index.v1+json") or media_type == "application/vnd.oci.image.index.v1+json" or media_type == "application/vnd.docker.distribution.manifest.list.v2+json":
                    try:
                        document = json.loads(raw)
                    except (UnicodeDecodeError, json.JSONDecodeError) as error:
                        raise VerificationError(f"OCI graph document is invalid JSON: {digest}") from error
                    if document.get("schemaVersion") != 2:
                        raise VerificationError(f"OCI graph document has an invalid schema: {digest}")
                    documents[digest] = document
                    children = document.get("manifests", [])
                    if children:
                        if not isinstance(children, list):
                            raise VerificationError(f"OCI index children are invalid: {digest}")
                        pending.extend(children)
                    for key in ("config",):
                        child = document.get(key)
                        if child is not None:
                            child_raw = verify_member(archive, child, f"{key} descriptor", capture=True)
                            assert child_raw is not None
                            if key == "config" and child.get("mediaType") in IMAGE_CONFIG_TYPES:
                                try:
                                    documents[child["digest"]] = json.loads(child_raw)
                                except (UnicodeDecodeError, json.JSONDecodeError) as error:
                                    raise VerificationError("image config is invalid JSON") from error
                    for layer in document.get("layers", []):
                        verify_member(archive, layer, "OCI layer descriptor")

            runnable = [
                (digest, document)
                for digest, document in documents.items()
                if digest in descriptors
                and descriptors[digest].get("mediaType") in IMAGE_MANIFEST_TYPES
                and document.get("config", {}).get("mediaType") in IMAGE_CONFIG_TYPES
            ]
            if len(runnable) != 1:
                raise VerificationError("OCI graph must contain exactly one runnable image manifest")
            image_manifest_digest, image_manifest = runnable[0]
            config_descriptor = image_manifest["config"]
            config_digest = require_digest(config_descriptor.get("digest"), "image config digest")
            config = documents.get(config_digest)
            if not isinstance(config, dict):
                raise VerificationError("runnable image config is missing from the verified graph")
            labels = config.get("config", {}).get("Labels", {}) or {}
            if labels.get("org.opencontainers.image.revision") != source_sha:
                raise VerificationError("OCI config revision label does not match the dispatch source SHA")
            if labels.get("org.opencontainers.image.source") != "https://github.com/HuGR-dev/corelink-runners":
                raise VerificationError("OCI config source label does not match this repository")

            provenance_found = any(
                verify_attestation_payload(archive, descriptor, documents.get(digest, {}), image_manifest_digest)
                for digest, descriptor in descriptors.items()
                if digest in documents
            )
            if not provenance_found:
                raise VerificationError("OCI graph has no SLSA provenance bound to the runnable image manifest")

            # Buildx metadata varies by exporter: fields can identify the archive
            # index or the runnable manifest. Reconcile fields when present while
            # deriving the authoritative digests from verified archive bytes.
            allowed_output_digests = {index_digest, image_manifest_digest}
            if "containerimage.digest" in metadata:
                metadata_digest = metadata["containerimage.digest"]
                if require_digest(metadata_digest, "BuildKit containerimage.digest") not in allowed_output_digests:
                    raise VerificationError("BuildKit output digest disagrees with the verified OCI graph")
            if "containerimage.config.digest" in metadata:
                metadata_config_digest = metadata["containerimage.config.digest"]
                if require_digest(metadata_config_digest, "BuildKit containerimage.config.digest") != config_digest:
                    raise VerificationError("BuildKit config digest disagrees with the verified OCI graph")
            if "containerimage.descriptor" in metadata:
                metadata_descriptor = metadata["containerimage.descriptor"]
                digest, size = descriptor_parts(metadata_descriptor, "BuildKit containerimage.descriptor")
                if digest == index_digest:
                    expected_size = len(index_raw)
                elif digest == image_manifest_digest:
                    expected_size = descriptors[digest]["size"]
                else:
                    raise VerificationError("BuildKit descriptor digest disagrees with the verified OCI graph")
                if size != expected_size:
                    raise VerificationError("BuildKit descriptor size disagrees with the verified OCI graph")

    except (tarfile.TarError, OSError, KeyError, TypeError, AttributeError) as error:
        if isinstance(error, VerificationError):
            raise
        raise VerificationError(f"OCI archive is incomplete or malformed: {error}") from error

    return {
        "source_sha": source_sha,
        "image_ref": image_ref,
        "index_digest": index_digest,
        "manifest_digest": image_manifest_digest,
        "config_digest": config_digest,
        "provenance": "verified",
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("archive", type=Path)
    parser.add_argument("metadata", type=Path)
    parser.add_argument("image_ref")
    parser.add_argument("source_sha")
    args = parser.parse_args()
    result = verify_archive(args.archive, args.metadata, args.image_ref, args.source_sha)
    print(json.dumps(result, sort_keys=True))
    summary_path = Path(__import__("os").environ["GITHUB_STEP_SUMMARY"])
    with summary_path.open("a", encoding="utf-8") as summary:
        summary.write("### GitHub-hosted DevEnv build-only proof\n\n")
        summary.write(f"- Source commit: `{result['source_sha']}`\n")
        summary.write(f"- Local OCI image ref: `{result['image_ref']}`\n")
        summary.write(f"- Verified OCI index digest: `{result['index_digest']}`\n")
        summary.write(f"- Runnable manifest digest: `{result['manifest_digest']}`\n")
        summary.write(f"- Config digest: `{result['config_digest']}`\n")
        summary.write("- SLSA provenance: verified and bound to the runnable image manifest\n")
        summary.write("- Publication/config pin changes: none\n")


if __name__ == "__main__":
    try:
        main()
    except VerificationError as error:
        raise SystemExit(str(error)) from error
