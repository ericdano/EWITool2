"""Fit filter/amp behaviour from a calibrate `results.csv` made with the
plain-saw test patches from make_test_patches.py.

Usage: python3 fit_filters.py <results.csv>

Prints the pitch-tracking high-pass ratio (filter-off patch), level vs
breath, and for each take the 2-pole low-pass cutoff/Q that best explains
the recorded harmonic levels.
"""
import csv, math, sys, numpy as np
rows=[r for r in csv.DictReader(open(sys.argv[1])) if r['source']=='ewi']
H=16
def harm(r): return np.array([float(r[f'h{k}']) for k in range(1,H+1)])
ks=np.arange(1,H+1)
saw_db=-20*np.log10(ks)
def hp2(k,beta): return 20*np.log10(k**2/np.sqrt(k**4+beta**4))
def lp2(f,fc,Q):
    r=f/fc; return -10*np.log10((1-r*r)**2+(r/Q)**2)
def by(name): return [r for r in rows if r['name'].startswith(name)]
nyq=22050*0.9

print("== B: filter off, note sweep at breath 0.9: fit key-tracked 2-pole HPF ratio beta")
betas=[]
for r in by('T filter off'):
    if r['breath']!='0.90': continue
    f0=float(r['f0']); h=harm(r); ok=(ks*f0<nyq)&(h>-60)
    best=min(((np.mean(((h-saw_db)-(hp2(ks,b)-hp2(1,b)))[ok][:10]**2),b) for b in np.linspace(0.1,3,291)))
    dev=(h-saw_db)[:8]
    print(f" note {r['note']} f0 {f0:7.1f}  beta {best[1]:.2f} rms err {math.sqrt(best[0]):.2f} dB  dev {np.round(dev,1)}")
    betas.append(best[1])
beta=float(np.median(betas)) if betas else 1.2
print(" median beta",beta)

print("== B: level vs breath (note 60)")
lv=[(float(r['breath']),float(r['rms_db'])) for r in by('T filter off') if r['note']=='60']
lv.sort(); top=lv[-1][1]
for b,d in lv: print(f" breath {b:.1f}: {d-top:6.1f} dB   (b^1 {20*math.log10(b):6.1f}, b^2 {40*math.log10(b):6.1f})")

def fit_lp(r):
    f0=float(r['f0']); h=harm(r); src=saw_db+hp2(ks,beta)-hp2(1,beta)
    ok=(ks*f0<nyq)&(h>-70)
    best=None
    for lfc in np.linspace(math.log(40),math.log(16000),300):
        fc=math.exp(lfc)
        for Q in [0.5,0.6,0.707,0.85,1,1.3,1.7,2.2,3,4,5,7,10,14,20]:
            pred=src+lp2(ks*f0,fc,Q); pred-=pred[0]
            e=np.mean(((h-pred)[ok])**2)
            if best is None or e<best[0]: best=(e,fc,Q)
    return best
def note_of(hz): return 69+12*math.log2(hz/440)
for title,name in [('A cutoff (bmod 0, kf 64)','T cutoff'),('C bmod (freq 80)','T bmod'),('D keyfollow (freq 84)','T kf'),('E q (freq 72)','T q')]:
    print("==",title)
    for r in by(name):
        e,fc,Q=fit_lp(r)
        print(f" {r['name']:14} note {r['note']} breath {r['breath']}: fc {fc:7.0f} Hz = note {note_of(fc):5.1f}  Q {Q:5.2f}  err {math.sqrt(e):4.1f} dB  level {float(r['rms_db']):6.1f}")
