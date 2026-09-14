# AWS monitor infrastructure review packet

CloudFormation templates only; nothing here has been deployed. Intended accounts:
monitor `975306274105`, sensitivity `286590629898`, verifier `888348805607`,
all in `us-east-1`.

Foundation stacks create durable state, encrypted/WORM storage, roles, keys and
(for monitor) ECR. Runtime requires a real immutable ECR digest through
`ImageUri`; there is no placeholder image. Deploy monitor and sensitivity
foundations, publish/scan the image, create the monitor runtime (including the
cross-account Lambda permission), then create the sensitivity schedule runtime.
No template creates subscriptions or email destinations. Outputs expose ARNs only. This packet is not deployment
evidence or O-MONITORHOST acceptance.

```sh
./validate.sh
aws cloudformation validate-template --template-body file://monitor-foundation.yaml
for f in monitor-foundation.yaml monitor-runtime.yaml sensitivity-foundation.yaml \
         sensitivity-runtime.yaml verifier-foundation.yaml; do
  aws cloudformation validate-template --region us-east-1 \
    --cli-connect-timeout 5 --cli-read-timeout 10 --template-body file://"$f" >/dev/null
done
aws cloudformation create-change-set --stack-name corelink-monitor-foundation \
  --change-set-name review-$(date +%Y%m%d%H%M%S) --change-set-type CREATE \
  --template-body file://monitor-foundation.yaml --capabilities CAPABILITY_NAMED_IAM
```
