#!/usr/bin/env python3
"""Copies architecture.mmd into the README between the architecture markers."""
import pathlib
import re

root = pathlib.Path(__file__).resolve().parent.parent
contract = (root / "architecture.mmd").read_text().rstrip("\n")
readme = root / "README.md"
block = f"<!-- architecture:start -->\n```mermaid\n{contract}\n```\n<!-- architecture:end -->"
text = readme.read_text()
updated, count = re.subn(
    r"<!-- architecture:start -->.*?<!-- architecture:end -->",
    lambda _: block,
    text,
    flags=re.DOTALL,
)
if count != 1:
    raise SystemExit("README.md needs exactly one architecture marker pair")
readme.write_text(updated)
