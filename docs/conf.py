"""Build the existing Markdown guides without compiling native readers."""

from pathlib import Path
from functools import cache
import os
import re
import tomllib
from urllib.parse import quote, unquote, urlsplit

from sphinx.errors import SphinxError

DOCS = Path(__file__).resolve().parent
ROOT = DOCS.parent
with (ROOT / "Cargo.toml").open("rb") as metadata_file:
    metadata = tomllib.load(metadata_file)["package"]

project = "dioptrin-io"
author = "Clemens Prescher"
copyright = "2026, Clemens Prescher"
release = metadata["version"]
version = release

extensions = ["myst_parser"]
source_suffix = {".md": "markdown"}
root_doc = "index"
language = "en"
nitpicky = True
myst_heading_anchors = 4
exclude_patterns = ["_build", "README.md"]

html_theme = "furo"
html_title = f"dioptrin-io {release} manual"
html_theme_options = {
    "source_repository": "https://github.com/CPrescher/dioptrin-io/",
    "source_branch": "main",
    "source_directory": "docs/",
}

linkcheck_retries = 2
linkcheck_timeout = 20
linkcheck_rate_limit_timeout = 30
linkcheck_workers = 2

# Keep large corpus reports and source files in GitHub. Only these small,
# provenance-preserving measurement files are bundled as manual downloads.
DOWNLOADS = set((DOCS / "benchmarks/2026-11-07-windows-io").glob("*.json"))
REPOSITORY = "https://github.com/CPrescher/dioptrin-io/"
LINK = re.compile(r"(?P<start>\[[^\]]*\]\()(?P<target>[^)\s]+)(?P<end>\))")
FENCE = re.compile(r"^\s*(`{3,}|~{3,})(.*)$")


@cache
def _html_labels(path):
    return set(re.findall(r'<a id="([^"]+)"></a>', path.read_text()))


def _render_markdown(text, source, document):
    """Resolve included README links and link evidence without copying corpora.

    Existing guides and generated coverage ledgers remain unchanged. Fenced
    examples are left intact except for Cargo's Rust-only `ignore` annotation.
    """
    def replace_link(match):
        target = match["target"]
        uri = urlsplit(target)
        if uri.scheme or uri.netloc:
            return match[0]
        path = (source.parent / unquote(uri.path)).resolve() if uri.path else source
        if not path.exists():
            raise SphinxError(f"{source}: missing link target {target}")
        if path.suffix == ".md" and uri.fragment in _html_labels(path):
            title = " ".join(match["start"][1:-2].split())
            return "{ref}`" + title + " <" + uri.fragment + ">`"
        if not uri.path:
            return match[0]
        if (path.suffix == ".md" and path.is_relative_to(DOCS)) or path in DOWNLOADS:
            href = Path(os.path.relpath(path, document.parent)).as_posix()
        else:
            kind = "tree" if path.is_dir() else "blob"
            href = f"{REPOSITORY}{kind}/main/{quote(path.relative_to(ROOT).as_posix())}"
        if uri.fragment:
            href += "#" + uri.fragment
        return match["start"] + href + match["end"]

    result = []
    prose = []
    fence = None

    def flush_prose():
        # Link labels may span lines in the source Markdown.
        chunk = re.sub(r'<a id="([^"]+)"></a>', r"(\1)=", "".join(prose))
        result.append(LINK.sub(replace_link, chunk))
        prose.clear()

    for line in text.splitlines(keepends=True):
        marker = FENCE.match(line)
        if marker:
            delimiter, language = marker.groups()
            if fence is None:
                flush_prose()
                fence = delimiter
                if language.strip() == "rust,ignore":
                    line = line.replace("rust,ignore", "rust", 1)
            elif delimiter[0] == fence[0] and len(delimiter) >= len(fence) and not language.strip():
                fence = None
            result.append(line)
        elif fence is not None:
            result.append(line)
        else:
            prose.append(line)
    flush_prose()
    return "".join(result)


def _source_read(app, docname, content):
    path = Path(app.env.doc2path(docname)).resolve()
    content[0] = _render_markdown(content[0], path, path)


def _include_read(app, relative_path, parent_docname, content):
    source = (DOCS / relative_path).resolve()
    document = Path(app.env.doc2path(parent_docname)).resolve()
    content[0] = _render_markdown(content[0], source, document)


def setup(app):
    app.connect("source-read", _source_read)
    app.connect("include-read", _include_read)
