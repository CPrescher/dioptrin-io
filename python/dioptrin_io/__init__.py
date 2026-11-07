"""Open detector files and read Images containing NumPy data and source headers.

Stored axes, original counts and reserved values are preserved. Arrays use native
byte order; each Image carries matching header fields and additional metadata.
"""

from ._native import Image, Reader, __version__

__all__ = ["Image", "Reader", "__version__", "open"]


def open(path, *, dataset=None):
    """Open an immutable file snapshot for frame-by-frame reading.

    ``path`` accepts strings, bytes and path-like objects. Format detection uses
    file content. Generic HDF5 files require an explicit ``dataset`` path; known
    Eiger/LImA/Lambda conventions can be discovered. External HDF5 links are
    rejected. Close the reader explicitly or use it as a context manager.

    ``reader.nframes`` exposes the detected frame count. Opening prepares the
    reader and indexes metadata; it does not load the complete image stack into
    memory. ``reader.read(frame=0)`` decodes one selected frame as an Image with
    ``.data`` (a NumPy array), ``.header`` (parsed source header fields) and
    ``.metadata`` (additional source and layout information).
    ``reader[index]`` is equivalent to ``reader.read(index)``. Iteration yields
    Images in stored order, decoding only as each frame is requested. Returned
    Images remain usable after the reader is closed.
    Compressed outer wrappers may be staged to a temporary file on disk.
    """
    return Reader(path, dataset=dataset)
