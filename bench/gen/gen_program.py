#!/usr/bin/env python3
"""Self-contained synthetic program generator.

Emits a valid incc program with one global (G), one const (C), and N functions
f0..fN-1 in the same style as the committed bench/programs/gen_*.src files:
function fN calls lower-index functions, accumulates into a local x, and ends
with `return x;` so bench/run.py's single-statement body edit keeps working.
"""
import random
import sys


def gen(N, seed):
    """Return program text with N functions."""
    N = max(N, 10)  # main() below calls f0..f9, so keep at least 10.
    random.seed(int(seed))
    out = ["// generated", "", "int G = 0;", "", "const int C = 7;", ""]
    has_param = []
    for fi in range(N):
        hp = random.random() < 0.5
        has_param.append(hp)
        out.append(f"int f{fi}(int p0) {{" if hp else f"int f{fi}() {{")
        out.append("    int x = 0;")
        if hp:
            out.append("    x = p0;")
        for _ in range(random.randint(6, 9)):
            r = random.random()
            if r < 0.10 and fi > 0:
                callee = random.randrange(fi)
                arg = "1" if has_param[callee] else ""
                out.append(f"    x = x + f{callee}({arg});")
            elif r < 0.25:
                out.append("    if (x > 10) { x = x - 1; } else { x = x + 1; }")
            elif r < 0.35:
                out.append("    while (x < 5) { x = x + 1; }")
            elif r < 0.85:
                out.append(f"    x = x + {random.randint(1, 5)};")
            elif r < 0.95:
                out.append(f"    x = x * {random.randint(2, 3)};")
            else:
                src = "G" if random.random() < 0.5 else "C"
                out.append(f"    x = x + {src};")
        out += ["    return x;", "}", ""]
    out.append("int main() {")
    out.append("    int s = 0;")
    for fi in range(10):
        arg = "1" if has_param[fi] else ""
        out.append(f"    s = s + f{fi}({arg});")
    out += ["    return s;", "}"]
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    N = int(sys.argv[1]) if len(sys.argv) > 1 else 50
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
    print(gen(N, seed), end="")
