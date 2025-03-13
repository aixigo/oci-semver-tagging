# Semantic Version Tagging for OCI Images

```
Usage: oci-semver-tagging [OPTIONS] --user <USER> <--password-stdin|--password-env <ENV>> <IMAGE> [TAG_VERSION]

Arguments:
  <IMAGE>        The image that shall be tagged with semantic version tags
  [TAG_VERSION]  The version that the image will be tagged with. If not specified, the version will be parsed from the image's tag

Options:
  -u, --user <USER>              The user that is able to login to the registry
      --password-stdin           The user's password will be read from stdin
      --password-env <ENV>       The user's password will be read from the specified environment variable
  -t, --tag-prefix <TAG_PREFIX>  A prefix that will be put in front of the tags to be pushed
  -d, --dry-run                  If the tool only outputs only what it would push
  -h, --help                     Print help
  -V, --version                  Print version
```
