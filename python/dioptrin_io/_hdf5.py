"""Explicit source choices for native HDF5 snapshot reads."""

from dataclasses import dataclass
from os import PathLike


@dataclass(frozen=True)
class FileChoice:
    owner: str | PathLike
    stored_filename: str
    actual_file: str | PathLike


@dataclass(frozen=True)
class PrintfMapping:
    dataset: str
    mapping_index: int
    stored_filename: str
    declared_dataset: str
    blocks: int
    source_file: str | PathLike | None = None


@dataclass(frozen=True)
class Hdf5Sources:
    """Resolve declared VDS/raw sources relative to their physical owner."""

    follow_external_links: bool = False
    declared_fill: bool = False
    file_choices: tuple[FileChoice, ...] = ()
    printf_mappings: tuple[PrintfMapping, ...] = ()


class Hdf5SourcesRequired(OSError):
    """Exact missing files or finite printf block counts needed to open a reader."""

    def __init__(self, requirements):
        self.requirements = requirements
        super().__init__(f"HDF5 sources require caller choices: {requirements!r}")
