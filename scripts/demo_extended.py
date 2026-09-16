#!/usr/bin/env python3
"""Executable acceptance smoke for Rover's expanded local platform.
All agents are explicitly owned deterministic fixtures. HTTP/TLS protocol tests
are local; this is not a live provider, sandbox, or hosted CI certification.
"""
from __future__ import annotations
import argparse, json, os, pathlib, pty, select, shutil, signal, subprocess, tempfile, time
from datetime import datetime, timezone
P=pathlib.Path
TERMINAL={'CANDIDATE_READY','REVIEW_READY','CHECKS_BLOCKED','FAILED','ERROR','CANCELLED','TIMED_OUT','LOST','CONFLICT'}
HARNESS='''import pathlib,sys,xml.etree.ElementTree as E
ok=pathlib.Path('value').read_text()=='fixed'
s=E.Element('testsuite',tests='1'); c=E.SubElement(s,'testcase',name='regression')
if not ok: E.SubElement(c,'failure',message='EXPECTED_DEFECT').text='the expected value is wrong'
pathlib.Path('.rover-results').mkdir(exist_ok=True)
E.ElementTree(s).write('.rover-results/test.xml')
sys.exit(0 if ok else 1)
'''
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--binary',default='bin/rover');ap.add_argument('--report');ap.add_argument('--keep',action='store_true');args=ap.parse_args()
 binary=str(P(args.binary).resolve());root=P(os.path.realpath(tempfile.mkdtemp(prefix='rvr-e2e-')));state=root/'state';clientstate=root/'client-state';procs=[];rows=[]
 env={k:v for k,v in os.environ.items() if not k.startswith('GIT_')};env.update(GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL=os.devnull,GIT_AUTHOR_NAME='Rover Test',GIT_AUTHOR_EMAIL='fixture@example.invalid',GIT_COMMITTER_NAME='Rover Test',GIT_COMMITTER_EMAIL='fixture@example.invalid',TERM='xterm-256color')
 def write(p,v): p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(v) if not isinstance(v,str) else v);return p
 def git(repo,*argv):return subprocess.check_output(['git','-c','core.hooksPath='+os.devnull,'-c','commit.gpgsign=false','-C',str(repo),*argv],env=env,stderr=subprocess.PIPE,text=True).strip()
 def init(name,files,cfg):
  repo=root/name;repo.mkdir();files=dict(files);files['.rover/config.json']=json.dumps(cfg)
  for p,b in files.items():write(repo/p,b)
  git(repo,'init','-b','main');git(repo,'add','-A');git(repo,'commit','-m','owned test fixture');return repo
 def call(*argv,codes=(0,),st=state,timeout=45):
  p=subprocess.run([binary,'--state',str(st),*map(str,argv),'--json'],env=env,text=True,capture_output=True,timeout=timeout)
  if p.returncode not in codes:raise AssertionError(f'{argv}: {p.returncode}\n{p.stdout}\n{p.stderr}')
  try:return json.loads(p.stdout)
  except Exception as e:raise AssertionError(f'not JSON: {p.stdout!r} {p.stderr!r}') from e
 def passed(name,detail):rows.append({'scenario':name,'status':'passed','detail':detail});print('PASS',name,flush=True)
 def wait_task(tid,st=state):
  until=time.monotonic()+25
  while time.monotonic()<until:
   v=call('status','--id',tid,st=st)
   if v['status'] in TERMINAL:return v
   time.sleep(.05)
  raise AssertionError('task timeout '+tid)
 def spec(repo,argv,**kw):return dict(schema='rover/v1alpha1',objective='owned fixture',repository=str(repo),base='HEAD',argv=argv,timeout='30s',auto_verify=True,**kw)
 try:
  cfg={'schema':'rover/v1alpha1','checks':[{'id':'unit','argv':['python3','test_regression.py'],'timeout':'3s','required':True,'parser':'junit','min_tests':1,'report_path':'.rover-results/test.xml'}],'policy':{'require_review':True}}
  repo=init('repair',{'value':'broken','test_regression.py':HARNESS,'AGENTS.md':'Keep existing instructions.\n'},cfg)
  task=spec(repo,['python3','-c','print("first attempt leaves defect")'],max_attempts=2,repair_argv=['python3','-c','from pathlib import Path;Path("value").write_text("fixed")'])
  taskfile=write(root/'repair.json',task);t=call('task','run','--file',taskfile,'--allow-local','--key','repair');t=wait_task(t['id'])
  assert t['status']=='REVIEW_READY' and len(t['attempts'])==2,t
  old=call('report','--id',t['attempts'][0]['investigation']);assert old['decision']=='BLOCKED'
  passed('bounded_repair_preserves_attempts','A failed candidate remains recorded before a separately verified repair.')
  inv=call('report','--id',t['investigation_id']);assert (repo/'value').read_text()=='broken'
  patch=call('diff','--id',inv['id']);assert 'fixed' in patch['patch'];passed('applicable_snapshot_diff','Diff comes from retained snapshots; original checkout remains unchanged.')
  replay=call('replay','--id',inv['id'],'--allow-local',codes=(3,));assert replay['id']!=inv['id'] and replay['candidate']==inv['candidate'];passed('exact_replay','Stored candidate and configuration reused, new investigation preserved.')
  reg=write(root/'regression.json',{'schema':'rover/v1alpha1','check_id':'unit','test_paths':['test_regression.py'],'test_id':'regression','failure_contains':'EXPECTED_DEFECT'})
  pr=call('prove','regression','--repo',repo,'--base-snapshot',inv['base'],'--candidate',inv['candidate'],'--file',reg,'--allow-local');assert pr['assessment']=='SUPPORTED_WITHIN_SCOPE'
  passed('counterfactual_cli','Named expected failure on base, passing same testcase on candidate.')
  mut=write(root/'mutations.json',{'schema':'rover/v1alpha1','check_ids':['unit'],'mutations':[{'id':'reintroduce','path':'value','before':'fixed','after':'broken'}]})
  m=call('mutate','--repo',repo,'--base-snapshot',inv['base'],'--candidate',inv['candidate'],'--file',mut,'--allow-local');assert m['results'][0]['result']=='KILLED',m;passed('mutation_cli','A seeded reintroduced defect is detected; not an exhaustive test-adequacy claim.')
  search=call('context','search','--snapshot',inv['candidate'],'--query','fixed');assert search['matches'];passed('snapshot_context','Search returns snapshot and source references.')
  note=call('memory','put','--snapshot',inv['candidate'],'--text','fixture-only note','--origin','human','--ttl','1h')
  notes=call('memory','list','--snapshot',inv['candidate']);assert any(n['id']==note['id'] for n in notes);passed('scoped_memory','Explicit expiring notes are retrievable within their project.')
  before=(repo/'AGENTS.md').read_bytes();preview=call('integrate','--repo',repo,'--agent','generic');assert (repo/'AGENTS.md').read_bytes()==before
  receipt=call('integrate','--repo',repo,'--agent','generic','--apply');assert (repo/'AGENTS.md').read_bytes()!=before
  call('integrate','--undo',receipt['id']);assert (repo/'AGENTS.md').read_bytes()==before;passed('reversible_agent_instructions','Preview, approved append, and hash-checked undo preserve user content.')
  # True PTY task; fixture command reads the actual controlling terminal.
  pts=spec(repo,['/bin/sh','-c','printf "PTY_READY\\n"; read answer; printf "RECEIVED:%s\\n" "$answer"'],agent='generic-pty',interactive=True);pts['auto_verify']=False
  pt=call('task','run','--file',write(root/'pty.json',pts),'--allow-local');until=time.monotonic()+5
  while time.monotonic()<until:
   status=call('status','--id',pt['id'])
   if status.get('socket') and P(status['socket']).exists():break
   time.sleep(.05)
  import socket,base64
  con=socket.socket(socket.AF_UNIX);con.settimeout(3);con.connect(status['socket']);stream=con.makefile('rwb',buffering=0)
  stream.write((json.dumps({'type':'input','data':base64.b64encode(b'hello\n').decode()})+'\n').encode());captured=b''
  try:
   while True:
    line=stream.readline()
    if not line:break
    frame=json.loads(line)
    if frame.get('type')=='output':captured+=base64.b64decode(frame.get('data',''))
    if b'RECEIVED:hello' in captured:break
  finally:stream.close();con.close()
  assert b'RECEIVED:hello' in captured,captured;assert wait_task(pt['id'])['status']=='CANDIDATE_READY';passed('actual_pty_task','Actual terminal child received input and emitted final output.')
  # Human TUI runs inside a real terminal; q must exit and restore terminal.
  master,slave=pty.openpty();tu=subprocess.Popen([binary,'--state',str(state),'tui'],stdin=slave,stdout=slave,stderr=slave,env=env);procs.append(tu);os.close(slave);view=b'';until=time.monotonic()+5
  while time.monotonic()<until:
   ready,_,_=select.select([master],[],[],.2)
   if ready:
    try:view+=os.read(master,65536)
    except OSError:break
   if b'ROVER' in view.upper():break
  os.write(master,b'q');tu.wait(timeout=5);os.close(master);assert tu.returncode==0 and b'ROVER' in view.upper(),view
  passed('keyboard_tui','Real terminal rendered the dashboard and exited on q.')
  # File-disjoint parallel tasks and a dependent integration task.
  wc={'schema':'rover/v1alpha1','checks':[{'id':'command','argv':['/usr/bin/true'],'timeout':'2s','required':True,'parser':'exit-code'}],'policy':{'require_review':True}}
  wr=init('workflow',{'README.md':'workflow fixture'},wc)
  def node(n,cmd,deps=()):return {'id':n,'depends_on':list(deps),'task':{'objective':n,'argv':['/bin/sh','-c',cmd],'timeout':'5s','auto_verify':True}}
  flow={'schema':'rover/v1alpha1','objective':'combine explicit fixture tasks','repository':str(wr),'base':'HEAD','timeout':'30s','max_parallel':2,'allow_unreviewed_handoffs':True,'nodes':[node('a','printf a > a.txt'),node('b','printf b > b.txt'),node('join','test -f a.txt && test -f b.txt && printf joined > combined.txt',('a','b'))]}
  fp=write(root/'flow.json',flow);fr=call('workflow','run','--file',fp,'--allow-local','--key','flow');until=time.monotonic()+20
  while time.monotonic()<until:
   fr=call('workflow','status','--id',fr['id'])
   if fr['status'] in TERMINAL:break
   time.sleep(.05)
  assert fr['status']=='REVIEW_READY' and fr['investigation'],fr
  again=call('workflow','run','--file',fp,'--allow-local','--key','flow');assert again['id']==fr['id']
  passed('detached_parallel_workflow','Persistent DAG, dependency snapshot handoff, full integration checks, and idempotent dispatch.')
  # JSON-only MCP stdio (not a live agent host).
  mp=subprocess.Popen([binary,'--state',str(state),'mcp','--repo',str(wr)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env);procs.append(mp)
  def rpc(v):mp.stdin.write(json.dumps(v)+'\n');mp.stdin.flush()
  rpc({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'fixture','version':'1'}}});assert json.loads(mp.stdout.readline())['result']['protocolVersion']=='2025-11-25'
  rpc({'jsonrpc':'2.0','method':'notifications/initialized'});rpc({'jsonrpc':'2.0','id':2,'method':'tools/list','params':{}});tools=json.loads(mp.stdout.readline())['result']['tools'];assert tools and not any(x['name']=='rover_verify' for x in tools)
  mp.stdin.close();mp.wait(timeout=5);assert mp.returncode==0;passed('mcp_stdio_cli','Initialization, read-only tool discovery, and clean shutdown use real pipes.')
  # Real local HTTP server and separately launched remote CLI.
  readyfile=root/'ready.json';errlog=open(root/'server.log','w');server=subprocess.Popen([binary,'--state',str(state),'serve','--repo',str(wr),'--enable-execution','--allow-local','--ready-file',str(readyfile)],stdout=subprocess.DEVNULL,stderr=errlog,env=env);procs.append(server)
  until=time.monotonic()+8
  while not readyfile.exists() and time.monotonic()<until:
   if server.poll() is not None:raise AssertionError((root/'server.log').read_text())
   time.sleep(.05)
  endpoint=json.loads(readyfile.read_text())['endpoint']
  granted=call('grant','create','--repo',wr,'--tool','rover_status','--tool','rover_task_run','--tool','rover_report','--tool','rover_inspect','--note','owned integration fixture')
  token=root/'token';token.write_text(granted['token']);token.chmod(0o600)
  call('remote','node-add','--node','fixture','--endpoint',endpoint,'--token-file',token,st=clientstate)
  catalog=call('remote','tools','--node','fixture',st=clientstate);assert any(t['name']=='rover_task_run' for t in catalog['tools'])
  argfile=write(root/'remote-args.json',{'objective':'remote-control fixture','argv':['python3','-c','from pathlib import Path;Path("remote.txt").write_text("done")'],'timeout':'5s','key':'remote-once'})
  rt=call('remote','call','--node','fixture','--tool','rover_task_run','--arguments',argfile,st=clientstate)['structuredContent'];done=wait_task(rt['id']);assert done['status']=='REVIEW_READY',done
  repeat=call('remote','call','--node','fixture','--tool','rover_task_run','--arguments',argfile,st=clientstate);assert repeat['structuredContent']['id']==rt['id']
  passed('remote_cli_actual_task','A separate CLI authenticated to a local server, submitted a real detached task, and safely repeated its key.')
  call('grant','revoke','--repo',wr,'--id',granted['grant']['id'])
  denied=call('remote','tools','--node','fixture',st=clientstate,codes=(2,));assert 'error' in denied;passed('remote_grant_revocation','Revoked bearer grant denied subsequent remote requests.')
  server.send_signal(signal.SIGINT);server.wait(timeout=5);errlog.close()
  # Keys/signatures are local operator assertions, not independent certification.
  priv,pub=root/'sign.pem',root/'sign.pub';call('attest','keygen','--key',priv,'--public-key',pub)
  envfile=root/'attestation.json';signed=call('attest','sign','--id',inv['id'],'--key',priv,'--out',envfile)
  valid=call('attest','verify','--file',envfile,'--public-key',pub);assert valid['signature_valid']
  passed('signed_evidence','Retained investigation signed and verified with an explicitly supplied trusted public key.')
  backup=root/'backup';call('backup','--to',backup);call('backup-check','--from',backup);restored=root/'restored';call('restore','--from',backup,'--to',restored)
  ri=call('report','--id',inv['id'],st=restored);assert ri['candidate']==inv['candidate'];passed('backup_restore_cli','Online SQLite backup and hashed artifacts restore evidence without resuming processes or credentials.')
  summary={'schema':'rover.extended-smoke/v1','status':'passed','observed_at':datetime.now(timezone.utc).isoformat(),'scenario_count':len(rows),'scenarios':rows,'live_model_provider':False,'docker_live':False,'remote_host':'localhost only','fixture_directory':str(root) if args.keep else 'removed'}
  if args.report:write(P(args.report),summary)
  print(f'\n{len(rows)} extended CLI scenarios passed.',flush=True)
  if args.keep:print(root)
 finally:
  for p in procs:
   if p.poll() is None:
    p.terminate()
    try:p.wait(timeout=4)
    except subprocess.TimeoutExpired:p.kill();p.wait()
  if not args.keep:shutil.rmtree(root,ignore_errors=True)
if __name__=='__main__':main()
