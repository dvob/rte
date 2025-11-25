# rte

rte (rusty template executor) renders templated project directories using Minijinja.

For example if you have a directory `examples/mytemplate/` which contains files like this:
```
# {{ values.app_name }}

* Author: {{ values.author }}
```

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

## Why
There is already [Cookiecutter](https://github.com/cookiecutter/cookiecutter) for bootstrapping projects, why creating a new tool?
I hadn't played with Rust for a long time and this was a good opportunity. Also I was looking for a solution to test Backstage software templates (see `rte --backstage`).

## Usage

```
rte [OPTIONS] <SOURCE> <DESTINATION>
```

**Sources:** directory, `.tar.gz` archive, `gitlab://host/group/project[@ref]`, or `github://host/owner/repo[@ref]`

**Destinations:** directory or `.tar.gz` archive

**Options:**
- `-p, --parameters <FILE>` - Parameter file (YAML), can be used multiple times
- `-s, --set <KEY=VALUE>` - Set parameter directly, overrides file parameters
- `-f, --force` - Write into existing directory
- `--backstage` - Use Backstage syntax (`${{ }}` instead of `{{ }}`)
- `--parameters-on-root` - Don't wrap parameters under `values` key
- `--template-path <PATH>` - Template subdirectory within source (for archives/repos)
- `--gitlab-token <TOKEN>` - GitLab token (or set `GITLAB_TOKEN` env var)
- `--github-token <TOKEN>` - GitHub token (or set `GITHUB_TOKEN` env var)

**Examples:**
```bash
# From directory
rte -p params.yaml ./template ./output

# From GitLab
rte -p params.yaml gitlab://gitlab.com/group/project@main ./output

# From GitHub
rte -p params.yaml github://github.com/owner/repo@main ./output

# Backstage template from GitHub
rte --backstage -p params.yaml github://github.com/backstage/software-templates@main ./output
```
