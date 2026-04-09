# Benchmark Storage Sync Secrets Checklist

Use this checklist before enabling `.github/workflows/benchmark-storage-sync.yml`.

## Common prerequisites

- Confirm benchmark report generation works in CI (`.github/workflows/performance.yml`).
- Decide provider (`s3`, `gcs`, or `azure`) and target path naming convention.
- Keep `id-token: write` permission enabled for OIDC-based auth.
- Prefer short-lived federated credentials over long-lived static keys.

## AWS S3 (OIDC)

Required repository/environment secrets:

- `AWS_ROLE_TO_ASSUME`
- `AWS_REGION`

Bootstrap steps:

1. Create IAM role trusted by GitHub OIDC provider.
2. Scope trust policy to this repo/workflow branch policy.
3. Grant least-privilege S3 permissions (`s3:ListBucket`, `s3:GetObject`, `s3:PutObject`, `s3:DeleteObject`) on target prefix.
4. Add role ARN + region to GitHub secrets.
5. Run workflow with `provider=s3`.

## Google Cloud Storage (OIDC)

Required repository/environment secrets:

- `GCP_WORKLOAD_IDENTITY_PROVIDER`
- `GCP_SERVICE_ACCOUNT`

Bootstrap steps:

1. Create Workload Identity Pool + Provider for GitHub OIDC.
2. Bind provider principal to service account with `roles/iam.workloadIdentityUser`.
3. Grant storage permissions to service account on target bucket/prefix.
4. Add provider resource name + service account email to GitHub secrets.
5. Run workflow with `provider=gcs`.

## Azure Blob (OIDC)

Required repository/environment secrets:

- `AZURE_CLIENT_ID`
- `AZURE_TENANT_ID`
- `AZURE_SUBSCRIPTION_ID`

Bootstrap steps:

1. Create app registration/service principal for GitHub OIDC federation.
2. Add federated credential scoped to repo/branch/workflow constraints.
3. Grant least-privilege storage data-plane role on target container (for blob sync).
4. Add client/tenant/subscription IDs to GitHub secrets.
5. Run workflow with `provider=azure`.

## Validation checklist

- Run one dry sync to a non-production prefix.
- Confirm `latest.*`, `history.json`, and timestamped reports upload correctly.
- Verify delete behavior does not remove unrelated objects.
- Verify dashboard assets render from the remote location.
- Document owner/rotation policy for each secret and identity binding.
