"""macOS evidence: is a PID that is still in use as a process-group ID ever handed to a new process?"""
import os, signal, subprocess, sys, time
def wrap(target, limit=400000):
    """Fork until the PID counter has wrapped past `target` twice or target was allocated."""
    hit, wraps, last, n = False, 0, 0, 0
    while wraps < 2 and n < limit and not hit:
        pid = os.fork()
        if pid == 0:
            os._exit(0)
        os.waitpid(pid, 0)
        n += 1
        if pid == target: hit = True
        if pid < last: wraps += 1
        last = pid
    return hit, wraps, n
leader = subprocess.Popen(['/bin/sleep', '3000'], process_group=0)
member = subprocess.Popen(['/bin/sleep', '3000'], process_group=leader.pid)
time.sleep(0.2)
pg = subprocess.run(['/bin/ps', '-o', 'pid=,pgid=', '-p', f'{leader.pid},{member.pid}'], capture_output=True, text=True).stdout.split()
print('leader', leader.pid, 'member', member.pid, 'ps', pg, flush=True)
leader.kill(); leader.wait()
print('leader reaped; group still has the member', flush=True)
t = time.time(); r = wrap(leader.pid); print('while group alive: pid reused=%s wraps=%s forks=%s %.0fs' % (*r, time.time()-t), flush=True)
# signalling the group now reaches exactly the member
os.killpg(leader.pid, signal.SIGKILL); member.wait(); print('member exit', member.returncode, flush=True)
t = time.time(); r = wrap(leader.pid); print('after group gone: pid reused=%s wraps=%s forks=%s %.0fs' % (*r, time.time()-t), flush=True)
