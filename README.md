# rte

rte (rusty template executor) renders templated project directories using Minijinja.

For example if you have a directory `examples/mytemplate/` which contains files like this:
```
# {{ values.app_name }}

* Author: {{ values.author }}
``

You can prepare a parameters file like this (`examples/params.yaml`):
```
app_name: myapp
author: John Doe
```

And then run `rte` like this:
```
rte -p examples/params.yaml examples/mytemplate output

rte -s app_name=myapp -s author="John Doe" examples/mytemplate output
```

This will go through all files in the directory, run them through Minijinja and put them under output.

## Usage

```
rte [OPTIONS] <SOURCE> <DESTINATION>
```

**Sources:** directory, `.tar.gz` archive, or `gitlab://host/group/project[@ref]`

**Destinations:** directory or `.tar.gz` archive

**Options:**
- `-p, --parameters <FILE>` - Parameter file (YAML), can be used multiple times
- `-s, --set <KEY=VALUE>` - Set parameter directly, overrides file parameters
- `-f, --force` - Write into existing directory
- `--backstage` - Use Backstage syntax (`${{ }}` instead of `{{ }}`)
- `--parameters-on-root` - Don't wrap parameters under `values` key
- `--gitlab-token <TOKEN>` - GitLab token (or set `GITLAB_TOKEN` env var)

**Examples:**
```bash
# From directory
rte -p params.yaml ./template ./output

# From GitLab
rte -p params.yaml gitlab://gitlab.com/group/project@main ./output

# Backstage template
rte --backstage -p params.yaml gitlab://gitlab.com/group/template ./output
```
