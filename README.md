# Semantic Version Tagging for OCI Images

```
Provides partial semver tagging for OCI image distribution so that a semver based image tags can also be provided with moving partial semver tags. For example, the latest release 1.2.3 can be available under 1 and 1.2 while there is also 1.1 and 1.0

Usage: oci-semver-tagging [OPTIONS] <COMMAND>

Commands:
  tag       Tags the given image with partial semantic version tags
  validate  Validates if the existing tags partially semver tagged according to the tag command
  help      Print this message or the help of the given subcommand(s)

Options:
  -u, --user <USER>          The user that is able to login to the registry
  -p, --protocol <PROTOCOL>  [default: https] [possible values: https, http]
      --password-stdin       The user's password will be read from stdin
      --password-env <ENV>   The user's password will be read from the specified environment variable
  -h, --help                 Print help
  -V, --version              Print version
```
