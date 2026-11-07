import pytest


@pytest.fixture
def h5py():
    return pytest.importorskip("h5py", reason="reference encoder wheel unavailable for this interpreter")
