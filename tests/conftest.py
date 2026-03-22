import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent

# Ensure the project root is on sys.path so `orchestrator` package is importable.
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))


def read_doc(name: str) -> str:
    return (ROOT / name).read_text(encoding="utf-8")