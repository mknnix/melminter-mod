import os

gitsha = os.popen('git rev-parse HEAD').read().strip()
print('GIT CURRENT SHA', repr(gitsha))

assert len( bytes.fromhex(gitsha) ) == 20

rs = 'build.rs'

d = open(rs, 'r').read()
ds = d.split('\n')

ptr = 0
while '//CODEADD//' not in ds[ptr]:
    ptr += 1
print('PTR', ptr)

ds[ptr] = 'let static_git_sha = "git.%s"; //CODEADD// by gitsha in code' % (gitsha,)

d2 = '\n'.join(ds)
print(ds)

open(rs, 'w').write(d2)
os.sync()

