#!/usr/bin/env python3
import argparse, csv, os, subprocess, tempfile, statistics

BIN = os.path.join(os.path.dirname(__file__), "..", "target", "debug", "incc")
BIN = os.path.abspath(BIN)
PROGDIR = os.path.join(os.path.dirname(__file__), "programs")

def run_build(src, out, cache, extra):
    cmd = [BIN, "build", src, "-o", out] + extra
    if AUDIT: cmd += ["--audit"]
    if cache is None:
        cmd += ["--no-cache"]
    else:
        cmd += ["--cache-dir", cache]
    r = subprocess.run(cmd, capture_output=True, text=True)
    timing = {}
    hits = misses = 0
    for line in (r.stdout + r.stderr).splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[2] == "ms":
            timing[parts[0]] = float(parts[1])
        if "hits=" in line:
            for tok in line.split():
                if tok.startswith("hits="): hits = int(tok.split("=")[1])
                if tok.startswith("misses="): misses = int(tok.split("=")[1])
    return r.returncode, timing, hits, misses

AUDIT=False
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--out", default="results/run.csv")
    ap.add_argument("--audit", action="store_true")
    args = ap.parse_args()
    global AUDIT
    AUDIT = args.audit
    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    configs = ["clean", "incremental", "file_level", "body_only"]
    Ns = [50, 200, 1000]
    rows = []
    for N in Ns:
        src = os.path.join(PROGDIR, f"gen_{N}.src")
        # body-edit version
        edit_src = os.path.join(tempfile.gettempdir(), f"gen_{N}_edit.src")
        s = open(src).read()
        s = s.replace("    return x;\n", "    return x + 1;\n", 1)
        open(edit_src, "w").write(s)
        for cfg in configs:
            for run in range(args.runs):
                d = tempfile.mkdtemp()
                cache = None if cfg == "clean" else os.path.join(d, "cache")
                extra = []
                if cfg == "body_only": extra = ["--body-only-key"]
                if cfg == "file_level": extra = ["--file-level-cache"]
                # populate cache for incremental configs with original
                if cfg != "clean":
                    run_build(src, os.path.join(d, "base.out"), cache, extra)
                rc, timing, hits, misses = run_build(edit_src, os.path.join(d, "edit.out"), cache, extra + ["--timings"])
                # re-run with timings by rebuilding? timing already captured if --timings passed; ensure
                # We didn't pass --timings above; rerun quickly? Instead pass --timings in all runs.
                total = sum(timing.values())
                rows.append({
                    "program": f"gen_{N}", "N": N, "config": cfg, "edit_type": "body_edit",
                    "edit_pct": 1, "run_id": run,
                    "parse": timing.get("parse", 0), "deps": timing.get("deps", 0),
                    "compile": timing.get("compile", 0), "link": timing.get("link", 0),
                    "total": total, "hits": hits, "misses": misses,
                    "audit_ok": 1 if rc == 0 else 0,
                })
    with open(args.out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)
    print(f"wrote {args.out} ({len(rows)} rows)")

if __name__ == "__main__":
    main()
