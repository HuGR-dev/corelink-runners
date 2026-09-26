#!/usr/bin/env python3
"""Verify an OCI archive against BuildKit digest metadata and measure its layers."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import sys
import tarfile
from pathlib import Path


def fail(message: str) -> int:
    print(f"verify_runner_image_oci_archive: {message}", file=sys.stderr)
    return 1


def digest_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def member_bytes(archive: tarfile.TarFile, name: str) -> bytes:
    member = archive.getmember(name)
    stream = archive.extractfile(member)
    if stream is None:
        raise ValueError(f"archive member is not a file: {name}")
    return stream.read()


def blob_name(digest: str) -> str:
    algorithm, separator, encoded = digest.partition(":")
    if algorithm != "sha256" or not separator or len(encoded) != 64:
        raise ValueError(f"unsupported or malformed digest: {digest}")
    return f"blobs/{algorithm}/{encoded}"


def member_digest(archive: tarfile.TarFile, name: str) -> str:
    stream = archive.extractfile(archive.getmember(name))
    if stream is None:
        raise ValueError(f"archive member is not a file: {name}")
    hasher = hashlib.sha256()
    while chunk := stream.read(1024 * 1024):
        hasher.update(chunk)
    return "sha256:" + hasher.hexdigest()


def expanded_layer_bytes(archive: tarfile.TarFile, name: str, media_type: str) -> int:
    stream = archive.extractfile(archive.getmember(name))
    if stream is None:
        raise ValueError(f"archive member is not a file: {name}")
    if media_type.endswith("+gzip"):
        expanded = gzip.GzipFile(fileobj=stream)
    elif media_type.endswith(".tar") or media_type.endswith(".tar+none"):
        expanded = stream
    else:
        raise ValueError(f"unsupported layer compression: {media_type}")
    total = 0
    while chunk := expanded.read(1024 * 1024):
        total += len(chunk)
    return total


def verify(archive_path: Path, metadata_path: Path, image_ref: str) -> int:
    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        manifest_digest = metadata["containerimage.digest"]
        config_digest = metadata["containerimage.config.digest"]
        expected_descriptor = metadata["containerimage.descriptor"]["digest"]
        if manifest_digest != expected_descriptor:
            return fail("BuildKit digest and descriptor metadata disagree")

        with tarfile.open(archive_path, mode="r:*") as archive:
            members = {member.name: member for member in archive.getmembers()}
            if "oci-layout" not in members or "index.json" not in members:
                return fail("BuildKit output is not an OCI image layout archive")
            layout = json.loads(member_bytes(archive, "oci-layout"))
            if layout.get("imageLayoutVersion") != "1.0.0":
                return fail("unsupported OCI layout version")
            index = json.loads(member_bytes(archive, "index.json"))
            descriptors = index.get("manifests")
            if not isinstance(descriptors, list) or not descriptors:
                return fail("OCI index has no image descriptor")
            names = {
                descriptor.get("annotations", {}).get("org.opencontainers.image.ref.name")
                for descriptor in descriptors
            }
            if image_ref not in names:
                return fail(f"OCI index does not preserve requested image ref {image_ref}")

            pending = list(descriptors)
            seen: set[str] = set()
            found_manifest = False
            found_config = False
            uncompressed_layers = 0
            while pending:
                descriptor = pending.pop()
                digest = descriptor.get("digest")
                if not isinstance(digest, str) or digest in seen:
                    continue
                seen.add(digest)
                raw = member_bytes(archive, blob_name(digest))
                if digest_bytes(raw) != digest:
                    return fail(f"OCI blob digest mismatch: {digest}")
                document = json.loads(raw)
                if isinstance(document.get("manifests"), list):
                    pending.extend(document["manifests"])
                    continue
                if digest == manifest_digest:
                    found_manifest = True
                config = document.get("config", {})
                if config.get("digest") == config_digest:
                    config_raw = member_bytes(archive, blob_name(config_digest))
                    if digest_bytes(config_raw) != config_digest:
                        return fail("image config bytes do not match BuildKit config digest")
                    found_config = True
                for layer in document.get("layers", []):
                    layer_digest = layer.get("digest")
                    layer_path = blob_name(layer_digest)
                    if member_digest(archive, layer_path) != layer_digest:
                        return fail(f"layer digest mismatch: {layer_digest}")
                    uncompressed_layers += expanded_layer_bytes(
                        archive, layer_path, layer.get("mediaType", "")
                    )

            if not found_manifest:
                return fail("BuildKit manifest digest is absent from the OCI archive")
            if not found_config:
                return fail("BuildKit config digest is absent from the OCI archive")

        receipt = {
            "image_ref": image_ref,
            "manifest_digest": manifest_digest,
            "config_digest": config_digest,
            "archive_bytes": archive_path.stat().st_size,
            "uncompressed_layer_bytes": uncompressed_layers,
        }
        print(json.dumps(receipt, sort_keys=True))
        return 0
    except (KeyError, OSError, tarfile.TarError, json.JSONDecodeError, ValueError, gzip.BadGzipFile) as error:
        return fail(str(error))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--metadata", required=True, type=Path)
    parser.add_argument("--image-ref", required=True)
    args = parser.parse_args()
    return verify(args.archive, args.metadata, args.image_ref)


if __name__ == "__main__":
    raise SystemExit(main())
