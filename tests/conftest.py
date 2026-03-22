from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def read_doc(name: str) -> str:
    return (ROOT / name).read_text(encoding="utf-8")