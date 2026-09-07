# Verifier witness runtime packet

This review-only template belongs in account `888348805607` and references the
existing verifier foundation outputs. It creates no duplicate table, bucket or
KMS key. `ImageUri` must be a real immutable digest. `WITNESS_CONFIG_JSON` is
`NoEcho` and must contain the frozen runtime contract; it is not a secret store.

Validate without applying resources:

```sh
python3 verifier-runtime.test.py
aws cloudformation validate-template --region us-east-1 \
  --cli-connect-timeout 5 --cli-read-timeout 10 \
  --template-body file://verifier-runtime.yaml
```

Deployment review must receive foundation outputs as explicit parameters:
`VerificationHeadTableArn`, `JournalBucketArn`, `WitnessKeyArn`, and
`VerifierRoleArn`; pass the monitor account's qualified role ARN as
`MonitorInvokerRoleArn`. `JournalPrefix` must be a canonical nonempty path
ending in `/`; this trailing boundary keeps `journal/` separate from
`journal-other/` in object and `ListBucketVersions` permissions. The permission targets only the published numeric
Lambda version. This packet will be source/configuration evidence, not deployment or
planned IAM/isolation evidence.
