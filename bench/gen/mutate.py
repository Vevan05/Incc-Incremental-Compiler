import random, re, sys

def mutate(src):
    # apply one random mutation, return new src or None
    mutations = ['ws','const','addfunc','reorder','body']
    m = random.choice(mutations)
    if m == 'ws':
        # add extra spaces/newlines
        return src.replace(';', '; ', 3)
    if m == 'const':
        # change a const int value
        n = None
        new, n = re.subn(r'const int (\w+) = (\d+);', lambda mm: f"const int {mm.group(1)} = {int(mm.group(2))+random.randint(1,5)};", src, count=1)
        return new if n > 0 else None
    if m == 'addfunc':
        # add a new unused function before main
        idx = src.rfind('int main()')
        if idx<0: return None
        f = f"\nint __g{random.randint(0,10000)}() {{ return {random.randint(0,9)}; }}\n"
        return src[:idx] + f + src[idx:]
    if m == 'reorder':
        # swap two top-level function definitions (naive: swap two 'int X(...){...}' blocks) - skip if complex
        return None
    if m == 'body':
        # change an integer literal inside a function by +1 (first occurrence)
        new, n = re.subn(r'return (\d+);', lambda mm: f"return {int(mm.group(1))+1};", src, count=1)
        return new if n>0 else None
    return None

if __name__=='__main__':
    random.seed(int(sys.argv[1]))
    src=open(sys.argv[2]).read()
    out=mutate(src)
    if out is None:
        print(src)
    else:
        print(out)
