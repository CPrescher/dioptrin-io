# Independent codec compatibility vectors

Synthetic integer patterns include zero, all-one, high-bit and mixed samples.
The expected pixels are computed independently in Rust. `shuffled.bin` was
encoded using stock Python Bitshuffle 0.5.2, not the patched native codec.
`manifest.json` records its hash, byte order and individual vector extents.

The regression compares both decoding and encoding against this independent
representation, for 1/2/4/8-byte samples and sizes covering vector loops,
multiple codec blocks and unshuffled tails. It runs on every architecture;
ARM builds exercise the changed NEON code. The reusable-scratch adapter is
also compared against these vectors with guarded buffers and invalid-size
rejections. No real detector data is included.

Regenerate with `scripts/generate_bitshuffle_neon_reference.py` in an isolated
environment containing stock Bitshuffle 0.5.2 and NumPy. The fixture license is
in `LICENSE`; the upstream codec keeps its own original MIT notices.
