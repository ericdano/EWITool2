"""Write single-parameter test patches (.syx) for calibrating ewitool-synth.

Usage (from any folder; files are written to the current directory):
    python3 make_test_patches.py <factory bank .syx>

Each series starts from a plain saw (no effects, no noise, filter 2 off) and
varies one setting. Record them with
    cargo run --release --example calibrate -- run ... --syx tests_<series>.syx
and fit with fit_filters.py. Results so far are recorded in
ewitool-synth/src/tuning.rs.
"""
import sys
src=open(sys.argv[1],'rb').read()
base=bytearray(src[0:206])  # Singing Saw as template
OSC=dict(octave=0,semitone=1,fine=2,beat=3,filler=4,saw=5,tri=6,sqr=7,pw=8,pwmf=9,pwmd=10,swd=11,swt=12,bdep=13,batt=14,bcurve=15,bthr=16,lvl=17)
FLT=dict(mode=0,freq=1,q=2,kf=3,bmod=4,lfof=5,lfod=6,lfob=7,lfot=8,swd=9,swt=10,bcurve=11)
SEC=dict(o1=47,o2=68,of1=89,of2=104,nf1=119,nf2=134,aa=149,noise=155,misc=161,amp=174,chorus=180,delay=192,rev=200)
MISC=dict(bend=0,step=1,bitevib=2,oflink=3,nflink=4,formant=5,xfade=6,keytrig=7,f10=8,chorus=9)
def setf(p,sec,field,v):
    idx={'o1':OSC,'o2':OSC,'of1':FLT,'of2':FLT,'nf1':FLT,'nf2':FLT,'misc':MISC}.get(sec)
    off=SEC[sec]+(idx[field] if idx else field)
    p[off]=v
def plain(name):
    p=bytearray(base)
    for k,v in dict(octave=64,semitone=64,fine=64,beat=64,saw=127,tri=0,sqr=0,pw=0,pwmf=0,pwmd=0,swd=64,swt=0,bdep=64,batt=0,bcurve=64,bthr=0,lvl=127).items(): setf(p,'o1',k,v)
    setf(p,'o2','lvl',0)
    for k,v in dict(mode=0,freq=84,q=5,kf=64,bmod=0,lfof=0,lfod=0,lfob=64,lfot=0,swd=64,swt=0,bcurve=64).items(): setf(p,'of1',k,v)
    setf(p,'of2','mode',4)
    for k,v in dict(bitevib=0,oflink=0,nflink=0,formant=0,xfade=0,keytrig=1,chorus=0).items(): setf(p,'misc',k,v)
    p[SEC['noise']+2]=0          # noise level
    p[SEC['amp']+0]=0; p[SEC['amp']+1]=100   # tremolo, master level
    p[SEC['delay']+3]=0; p[SEC['delay']+4]=127   # delay level 0, dry full
    p[SEC['rev']+0]=127; p[SEC['rev']+1]=0       # reverb dry full, level 0
    nm=name.encode()[:32].ljust(32,b' '); p[12:44]=nm
    return p
series={}
series['A_cutoff']=[]
for f in [48,60,72,84,96,108,120]:
    p=plain(f'T cutoff {f}'); setf(p,'of1','freq',f); series['A_cutoff'].append(p)
p=plain('T filter off'); setf(p,'of1','mode',4); series['B_nofilter']=[p]
series['C_bmod']=[]
for b in [32,64,96,127]:
    p=plain(f'T bmod {b}'); setf(p,'of1','freq',80); setf(p,'of1','bmod',b); series['C_bmod'].append(p)
series['D_keyfollow']=[]
for k in [52,64,76,88]:
    p=plain(f'T kf {k}'); setf(p,'of1','kf',k); series['D_keyfollow'].append(p)
series['E_q']=[]
for q in [5,30,60,90,120]:
    p=plain(f'T q {q}'); setf(p,'of1','q',q); setf(p,'of1','freq',72); series['E_q'].append(p)
for name,ps in series.items():
    open(f'tests_{name}.syx','wb').write(b''.join(ps))
    print(name,len(ps))
series2=[]
for c in [32,48,64,80,96]:
    p=plain(f'T curve {c}'); setf(p,'of1','freq',80); setf(p,'of1','bmod',64); setf(p,'of1','bcurve',c); series2.append(p)
open('tests_F_curve.syx','wb').write(b''.join(series2)); print('F_curve',len(series2))
series3=[]
for f in [60,80,100,120]:
    p=plain(f'T bf {f}'); setf(p,'of1','freq',f); setf(p,'of1','bmod',64); series3.append(p)
open('tests_G_bmodfreq.syx','wb').write(b''.join(series3)); print('G',len(series3))

# ---- Filter LFO (osc filter 1): plain saw, cutoff 80, no breath mod ----
def lfo(name, rate, depth, breath=64, thr=0):
    p = plain(name)
    setf(p, 'of1', 'freq', 80)
    for k, v in dict(lfof=rate, lfod=depth, lfob=breath, lfot=thr).items():
        setf(p, 'of1', k, v)
    return p
lfo_series = {
    'L1_rate': [lfo(f'T lfo rate {r}', r, 16) for r in [1, 8, 16, 24, 32, 48, 64, 96, 127]],
    'L2_depth': [lfo(f'T lfo depth {d}', 24, d) for d in [2, 4, 8, 16, 32, 64, 127]],
    'L3_breath': [lfo(f'T lfo breath {b}', 24, 16, breath=b) for b in [0, 32, 64, 96, 127]],
    'L4_thresh': [lfo(f'T lfo thr {t}', 24, 16, thr=t) for t in [0, 32, 64, 96, 127]],
}
lfo_series['L5_breath_detail'] = [
    lfo('T lb127 d4', 24, 4, breath=127),
    lfo('T lb127 d32', 24, 32, breath=127),
    lfo('T lb127 thr32', 24, 16, breath=127, thr=32),
    lfo('T lb127 thr64', 24, 16, breath=127, thr=64),
    lfo('T lb127 thr96', 24, 16, breath=127, thr=96),
    lfo('T lb0 thr64', 24, 16, breath=0, thr=64),
    lfo('T lb32 thr96', 24, 16, breath=32, thr=96),
]
for name, ps in lfo_series.items():
    open(f'tests_{name}.syx', 'wb').write(b''.join(ps))
    print(name, len(ps))
