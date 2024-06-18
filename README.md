# Jobber OAuth Handler

Use lambda to interface with jobber's authentication process to get tokens and store them for usage

## Release

- Set up the following lambda environment variables before releasing:
  - The `app_name=app_secret` (provided by Jobber when creating an app)
  - `S3_BUCKET` - Requires an S3 bucket in the same region as the lambda.
- Ensure the IAM role that the lambda uses has access to the `S3_BUCKET`.

Until I figure out how to install this on MacOS to compile to Linux, use the docker build to build the release.

```bash
docker buildx build --platform linux/amd64 .
```

Then install the lambda using docker:

```bash
docker run \
  -it \
  --rm \
  -v ~/.aws:/root/.aws \
  -e AWS_PROFILE=$AWS_PROFILE \
  `docker buildx build --platform linux/amd64 . -q` \
  cargo lambda deploy --iam-role arn:aws:iam::$AWS_ACCOUNT_ID:role/$IAM_ROLE jobber-oauth-handler
```

