# Jobber OAuth Handler

Use lambda to interface with jobber's authentication process to get tokens and store them for usage

## Release

Until I figure out how to install this on MacOS to compile to Linux, use the docker build to build the release. 


 ```bash
 docker build .
 ```

 Then install the lambda using docker:

 ```bash
docker run -it -v ~/.aws:/root/.aws -e AWS_PROFILE=$AWS_PROFILE --rm `docker build . -q` cargo lambda deploy --iam-role arn:aws:iam::$AWS_ACCOUNT_ID:role/$IAM_ROLE jobber-oauth-handler
 ```


