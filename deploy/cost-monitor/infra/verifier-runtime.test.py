from pathlib import Path
import re

T = Path(__file__).with_name("verifier-runtime.yaml").read_text()

assert "PackageType: Image" in T
assert "sha256:[0-9a-f]{64}" in T
assert "WITNESS_CONFIG_JSON: !Ref WitnessConfigJson" in T
assert "NoEcho: true" in T
assert "FunctionName: !Ref PublishedVersion" in T
assert "Principal: !Ref MonitorInvokerRoleArn" in T
assert "975306274105" in T and "888348805607" in T
assert "s3:ListBucketVersions" in T
assert "s3:GetObjectRetention" in T and "s3:PutObject" in T
assert "kms:GetPublicKey" in T and "kms:Sign" in T
assert "sns:" not in T and "AWS::CloudFront" not in T and "SecretsManager" not in T
assert "Principal: '*'" not in T
assert "FunctionName: !Ref WitnessFunction" not in T.split("  MonitorInvokePermission:", 1)[1]
assert not re.search(r"ImageUri:.*:latest", T, re.I)
print("verifier runtime policy assertions: PASS")
