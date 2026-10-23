# Reuse generator from earlier
import random, sys
sys.path.insert(0,'/tmp/opencode/bench')
# import gen module by exec
exec(open('/tmp/opencode/bench/gen.py').read().split('if __name__')[0])
if __name__=='__main__':
    N=int(sys.argv[1]); seed=int(sys.argv[2]) if len(sys.argv)>2 else 1
    print(gen(N,seed))
