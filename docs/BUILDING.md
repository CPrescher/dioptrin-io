# Building the manual

The manual uses Sphinx, MyST Markdown and Furo, with the same pinned documentation
toolchain as Dioptrin. Existing reader guides and generated coverage ledgers are
the source; no reader build, native dependencies or detector dataset is needed.
The displayed package version is read from `Cargo.toml`.

## Local build

From the repository root, using Python 3.13:

```sh
python3.13 -m venv .venv-docs
source .venv-docs/bin/activate
python -m pip install -r docs/requirements.txt
python -m sphinx -n -W --keep-going -b html docs docs/_build/html
```

On Windows use `py -3.13 -m venv .venv-docs` and activate with
`.venv-docs\Scripts\Activate.ps1` in PowerShell.

Open `docs/_build/html/index.html`, or serve the output with
`python -m http.server --directory docs/_build/html 8000`.
The Documentation workflow runs the strict build for pushes and pull requests.

## Source and evidence links

The site includes the README and changelog without maintaining separate copies.
`docs/conf.py` resolves their relative links during parsing. It also presents
Cargo's `rust,ignore` examples as Rust code and translates existing HTML anchors
in the generated gap ledger into validated MyST labels. Source guides and
generated ledgers remain unchanged.

Historical evidence, scripts and native source are linked to the repository;
large corpus reports are not copied into the site. The three small Windows
read/decode JSON records linked from the performance guide remain downloads
with their original hashes and provenance. Scientific/reference scopes stay
attached to each record's tested revision.

## Read the Docs

The published manual is at
[dioptrin-io.readthedocs.io](https://dioptrin-io.readthedocs.io/en/latest/).

The root `.readthedocs.yaml` uses Ubuntu 24.04, Python 3.13 and
`docs/requirements.txt`, with `docs/conf.py` and warnings treated as errors.
The hosted `latest` version tracks `main`. A signed GitHub incoming webhook
delivers repository updates for automatic builds. Project settings are managed
in [Read the Docs](https://app.readthedocs.org/projects/dioptrin-io/).
Release versions must contain this configuration;
the older `v0.1.0` tag does not gain this manual automatically. Development
benchmarks identify their measured commit separately from that release.

See the [Read the Docs configuration reference](https://docs.readthedocs.com/platform/stable/config-file/v2.html)
and [versioning guide](https://docs.readthedocs.com/platform/stable/versions.html).
