"""Compatibility entry point. Generates the original Abyss vortex resources.
Requires the existing Node.js runtime, no Python packages.
"""
from pathlib import Path
import subprocess

subprocess.run(["node", str(Path(__file__).resolve().parent.parent / "scripts" / "generate-brand.cjs")], check=True)
