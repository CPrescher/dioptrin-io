"""Fixed-mask float64 Eiger integration, bundled with dioptrin-io wheels.

The Rust reader crate remains independent of integration mathematics. This
compatibility import keeps existing Dioptas clients working without source builds.
"""
from dioptrin_io._native import (
    FusedHdf5Integrator, FusedResult, SourceImageMismatch, UnsupportedFusedLayout,
)

__all__ = ["FusedHdf5Integrator", "FusedResult", "SourceImageMismatch", "UnsupportedFusedLayout"]
