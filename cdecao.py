import json
import pathlib
import subprocess
import sys

from cdedb_droid.quick_partial_export import get_quick_partial_export

qpe_file = pathlib.Path(__file__).parent / "quick_partial_export.json"

with qpe_file.open("w", newline="", encoding="utf-8") as f:
    json.dump(get_quick_partial_export(), f)


args = ["cargo", "run", "--release", "--", str(qpe_file), "--cde"] + sys.argv[1:]
print(f"Running '{' '.join(args)}'.")
subprocess.call(args)
