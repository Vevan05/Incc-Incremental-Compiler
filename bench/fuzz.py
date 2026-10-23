import subprocess, os, random, sys

ROOT=os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN=os.path.join(ROOT,"target","debug","incc")
GEN=os.path.join(ROOT,"bench","gen","gen_program.py")
MUTATE=os.path.join(ROOT,"bench","gen","mutate.py")
N=int(sys.argv[1]) if len(sys.argv)>1 else 50
SEED=int(sys.argv[2]) if len(sys.argv)>2 else 1
random.seed(SEED)
iters=0; fails=0
for it in range(N):
    # generate base program
    base=subprocess.check_output(['python3',GEN,'20',str(random.randint(0,10**9))]).decode()
    d=f"/tmp/scratchpad/fuzz_{it}"
    os.makedirs(d,exist_ok=True)
    before_path=f"{d}/before.src"; after_path=f"{d}/after.src"
    open(before_path,'w').write(base)
    # mutate
    mutated=subprocess.check_output(['python3',MUTATE,str(random.randint(0,10**9)),before_path]).decode()
    open(after_path,'w').write(mutated)
    # populate cache with before
    cache=f"{d}/cache"
    os.makedirs(cache,exist_ok=True)
    subprocess.run([BIN,'build',before_path,'-o',f'{d}/out1','--cache-dir',cache],capture_output=True)
    # build after with audit
    r=subprocess.run([BIN,'build',after_path,'-o',f'{d}/out2','--cache-dir',cache,'--audit'],capture_output=True,text=True)
    iters+=1
    if r.returncode!=0:
        fails+=1
        print(f"FAIL iter {it}: {r.stderr.strip()[:200]}")
        # save failing case
        os.makedirs('/tmp/scratchpad/fuzz_failures',exist_ok=True)
        open(f'/tmp/scratchpad/fuzz_failures/case_{it}_before.src','w').write(base)
        open(f'/tmp/scratchpad/fuzz_failures/case_{it}_after.src','w').write(mutated)
        if fails>5:
            break
print(f"done: {iters} iters, {fails} failures")
