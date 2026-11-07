#!/usr/bin/env python3
"""Pinned native upgrade with complete offline backup and data-aware rollback."""
import argparse,datetime,fcntl,hashlib,importlib.util,json,os,pathlib,re,shutil,sqlite3,subprocess,tarfile,time,urllib.request

ROOT=pathlib.Path('/opt/stravia')
DATA=pathlib.Path('/var/lib/stravia')
BACKUPS=pathlib.Path('/var/backups/stravia')
BACKUPS.mkdir(parents=True,exist_ok=True,mode=0o700)
os.umask(0o077)
lock=open('/run/stravia-maint.lock','w');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)

def run(*args):subprocess.run(args,check=True)
def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  while chunk:=f.read(1024*1024):h.update(chunk)
 return h.hexdigest()
def ready():
 deadline=time.monotonic()+180
 while time.monotonic()<deadline:
  try:
   with urllib.request.urlopen('http://127.0.0.1:23471/readyz',timeout=3) as response:
    if response.status==200:return
  except Exception:pass
  time.sleep(1)
 raise RuntimeError('Stravia readiness check failed')
def snapshot_filter(member):
 # Staged candidates are rebuildable inputs, not the running instance. In
 # particular, archiving a staged host would duplicate it during rollback.
 for prefix in ['opt/stravia/compat/staged','opt/stravia/responses-compat/staged']:
  if member.name==prefix or member.name.startswith(prefix+'/'):return None
 return member

def snapshot(label):
 stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
 target=BACKUPS/(stamp+'-'+label+'.tar.gz')
 current=str((ROOT/'current').resolve())
 run('rc-service','stravia','stop')
 try:
  with tarfile.open(target,'w:gz') as archive:
   archive.add(DATA,arcname='var/lib/stravia')
   for name in ['/etc/init.d/stravia','/usr/local/sbin/stravia-server','/etc/nginx/stravia-proxy.inc','/etc/nginx/http.d/stravia.conf','/root/stravia-access.json','/root/stravia-client.py','/root/cache-stravia-assets.py','/usr/local/sbin/stravia-maint','/opt/stravia/compat','/opt/stravia/responses-compat','/usr/local/sbin/stravia-cc-compat']:
    p=pathlib.Path(name)
    if p.exists():archive.add(p,arcname=name.lstrip('/'),filter=snapshot_filter)
  target.chmod(0o600)
  manifest={'archive':str(target),'sha256':digest(target),'release':current}
  target.with_suffix('.json').write_text(json.dumps(manifest,indent=2))
 except BaseException:
  run('rc-service','stravia','start');raise
 return target,manifest
def switch(path):
 candidate=ROOT/'current.next';candidate.unlink(missing_ok=True);candidate.symlink_to(path,target_is_directory=True);os.replace(candidate,ROOT/'current')
def restore(archive,manifest):
 assert digest(archive)==manifest['sha256'],'Backup checksum mismatch'
 release=pathlib.Path(manifest['release']).resolve()
 assert release.is_relative_to(ROOT/'releases') and (release/'stravia-server').is_file()
 subprocess.run(['rc-service','stravia','stop'],check=False)
 retained=DATA.with_name('stravia.failed-'+datetime.datetime.now().strftime('%Y%m%d%H%M%S'))
 DATA.rename(retained)
 try:
  with tarfile.open(archive) as source:
   members=[m for m in source.getmembers() if m.name=='var/lib/stravia' or m.name.startswith('var/lib/stravia/')]
   assert members
   source.extractall('/',members=members,filter='data')
  # tarfile data filter strips ownership; restore dedicated service ownership.
  import pwd
  account=pwd.getpwnam('stravia')
  for path in [DATA,*DATA.rglob('*')]:os.chown(path,account.pw_uid,account.pw_gid)
  # Keep custom-plugin metadata synchronized with the restored plugin database,
  # including old backups that predate the managed compatibility component.
  compatibility=ROOT/'compat'
  if compatibility.exists():compatibility.rename(ROOT/('compat.retained-'+datetime.datetime.now().strftime('%Y%m%d%H%M%S')))
  responses_compatibility=ROOT/'responses-compat'
  if responses_compatibility.exists():responses_compatibility.rename(ROOT/('responses-compat.retained-'+datetime.datetime.now().strftime('%Y%m%d%H%M%S')))
  with tarfile.open(archive) as source:
   members=[m for m in source.getmembers() if m.name=='opt/stravia/compat' or m.name.startswith('opt/stravia/compat/') or m.name=='opt/stravia/responses-compat' or m.name.startswith('opt/stravia/responses-compat/') or m.name=='usr/local/sbin/stravia-cc-compat']
   source.extractall('/',members=members,filter='data')
  switch(release);run('rc-service','stravia','start');ready()
 except BaseException:
  print('Restore failed; original data retained at '+str(retained),flush=True);raise
 print(json.dumps({'restored':str(archive),'retained_newer_data':str(retained)}),flush=True)
def download(url,path,expected=None):
 if path.exists() and expected and digest(path)==expected:return
 temporary=path.with_suffix(path.suffix+'.part')
 with urllib.request.urlopen(urllib.request.Request(url,headers={'User-Agent':'MASUB-Stravia-Maintenance/1.0'}),timeout=180) as response,temporary.open('wb') as out:shutil.copyfileobj(response,out)
 if expected:assert digest(temporary)==expected,'Release checksum mismatch'
 os.replace(temporary,path)
def responses_upgrade_plan(version):
 directory=ROOT/'responses-compat'
 active=json.loads((directory/'manifest.json').read_text()) if (directory/'manifest.json').exists() else None
 if active:
  assert digest(ROOT/'current/stravia-server')==active['host_sha256'],'Running Responses-compatible host differs from its pinned digest'
  assert digest(directory/active['component'])==active['sha256'],'Managed Responses component checksum mismatch'
 staged=directory/'staged'/version
 if not (staged/'manifest.json').exists():
  raise RuntimeError('Responses compatibility is managed: stage the target-version host, Codex component, source.patch and manifest under '+str(staged)+' before upgrading')
 candidate=json.loads((staged/'manifest.json').read_text())
 assert candidate['upstream_tag']==version and candidate['vendor_id']==(active['vendor_id'] if active else 'openai-codex'),'Staged Responses compatibility target mismatch'
 assert candidate['version'].split('+')[0]==version.removeprefix('v'),'Staged Responses component version mismatch'
 assert set((active or {}).get('features',[])).issubset(set(candidate.get('features',[]))),'Staged host is missing managed compatibility features; retain request model mappings and other active patches'
 for key in ['host_binary','component']:
  assert pathlib.Path(candidate[key]).name==candidate[key],'Invalid staged Responses filename'
 assert digest(staged/candidate['host_binary'])==candidate['host_sha256'],'Staged Responses host checksum mismatch'
 assert digest(staged/candidate['component'])==candidate['sha256'],'Staged Responses component checksum mismatch'
 assert digest(staged/'source.patch')==candidate['source_patch_sha256'],'Staged Responses source checksum mismatch'
 return active,candidate,staged

def upgrade(version):
 assert re.fullmatch(r'v\d+\.\d+\.\d+',version),'Use a pinned stable version: vX.Y.Z'
 target=ROOT/'releases'/version;target.mkdir(parents=True,exist_ok=True)
 if (ROOT/'current').resolve().is_relative_to(target):
  print(json.dumps({'version':version,'already_current':True}),flush=True);return
 responses=responses_upgrade_plan(version) if (ROOT/'responses-compat/manifest.json').exists() or (ROOT/'responses-compat/staged'/version/'manifest.json').exists() else None
 with urllib.request.urlopen('https://api.github.com/repos/Stravia-AI/StraviaPlatform/releases/tags/'+version,timeout=30) as response:release=json.load(response)
 assert not release['draft'] and not release['prerelease']
 assets={a['name']:a for a in release['assets']}
 name='stravia-server-'+version+'-linux-x86_64-gnu.tar.gz'
 assert name in assets
 if shutil.disk_usage(ROOT).free<400*1024*1024:raise RuntimeError('At least 400 MiB free disk is required to stage and back up safely')
 sums=target/'SHA256SUMS';download(assets['SHA256SUMS']['browser_download_url'],sums,assets['SHA256SUMS'].get('digest','').removeprefix('sha256:') or None)
 checks={line.split()[-1].lstrip('*'):line.split()[0] for line in sums.read_text().splitlines() if line.strip()}
 archive=target/name;download(assets[name]['browser_download_url'],archive,checks[name])
 with tarfile.open(archive) as package:
  for member in package.getmembers():assert (target/member.name).resolve().is_relative_to(target)
  package.extractall(target,filter='data')
 binaries=[p for p in target.rglob('stravia-server') if p.is_file() and 'masub-responses' not in p.relative_to(target).parts];assert len(binaries)==1
 official_binary=binaries[0]
 binary=official_binary;binary.chmod(0o755)
 if responses:
  _,candidate,staged=responses
  custom=target/'masub-responses';custom.mkdir(mode=0o755,exist_ok=True);custom.chmod(0o755)
  temporary=custom/'stravia-server.next';temporary.unlink(missing_ok=True)
  os.link(staged/candidate['host_binary'],temporary)
  os.replace(temporary,custom/'stravia-server')
  binary=custom/'stravia-server';binary.chmod(0o755)
 loader=ROOT/'runtime/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2'
 run(str(loader),'--library-path',str(loader.parent),str(binary),'--version')
 # The verified official archive is retained. The staged custom host replaces
 # its unpacked stock binary, avoiding a second 190 MiB executable on this VPS.
 if responses:official_binary.unlink()
 spec=importlib.util.spec_from_file_location('client','/root/stravia-client.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
 client=module.Client().login()
 installed=client.call('GET','/api/v1/vendor-plugins')['data']
 if responses and responses[0]:
  active,_,_=responses
  plugin=next(p for p in installed if p['vendor_id']==active['vendor_id'])
  assert plugin['version']==active['version'] and plugin['status']=='ready','Managed Responses plugin differs from its manifest'
  with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
   assert db.execute('SELECT digest FROM vendor_plugins WHERE vendor_id=?',(active['vendor_id'],)).fetchone()[0]==active['sha256'],'Installed Responses plugin digest mismatch'
 compatibility=ROOT/'compat/manifest.json'
 managed=json.loads(compatibility.read_text()) if compatibility.exists() else None
 staged_manifest=ROOT/'compat/staged'/version/'manifest.json'
 replacement=json.loads(staged_manifest.read_text()) if staged_manifest.exists() else None
 if managed:
  assert digest(ROOT/'compat'/managed['component'])==managed['sha256'],'Managed compatibility component checksum mismatch'
  plugin=next(p for p in installed if p['vendor_id']==managed['vendor_id'])
  assert plugin['version']==managed['version'] and plugin['status']=='ready','Managed plugin differs from the pinned compatibility manifest'
  with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
   assert db.execute('SELECT digest FROM vendor_plugins WHERE vendor_id=?',(managed['vendor_id'],)).fetchone()[0]==managed['sha256'],'Installed compatibility component differs from its pinned digest'
 if replacement:
  assert managed and replacement['vendor_id']==managed['vendor_id'],'Staged compatibility vendor mismatch'
  assert replacement['upstream_tag']==version and replacement['version'].startswith(version.removeprefix('v')+'+'),'Staged compatibility version mismatch'
  assert pathlib.Path(replacement['component']).name==replacement['component'],'Invalid staged component name'
  assert digest(staged_manifest.parent/replacement['component'])==replacement['sha256'],'Staged compatibility component checksum mismatch'
  assert digest(staged_manifest.parent/'source.patch')==replacement['source_patch_sha256'],'Staged compatibility source checksum mismatch'
 components=[]
 for plugin in installed:
  vendor=plugin['vendor_id']
  if vendor=='base':continue
  if managed and vendor==managed['vendor_id']:continue
  if responses and vendor==responses[1]['vendor_id']:continue
  component='stravia-vendor-'+vendor+'-'+version+'.wasm'
  assert component in assets and component in checks,'Release lacks installed plugin: '+vendor
  path=target/component;download(assets[component]['browser_download_url'],path,checks[component]);components.append(path)
 backup,manifest=snapshot('before-'+version)
 try:
  switch(binary.parent);run('rc-service','stravia','start');ready()
  client=module.Client().login()
  if responses:
   _,candidate,staged=responses
   component=staged/candidate['component']
   preview=client.call('POST','/api/v1/vendor-plugins/import',component.read_bytes(),'application/wasm',timeout=600)['data']
   client.call('POST','/api/v1/vendor-plugins/confirm',{'preview_id':preview['id'],'allow_data_discard':False},timeout=600)
   plugin=next(p for p in client.call('GET','/api/v1/vendor-plugins')['data'] if p['vendor_id']==candidate['vendor_id'])
   assert plugin['status']=='ready' and plugin['version']==candidate['version'],'Rebuilt Responses plugin could not be loaded'
   directory=ROOT/'responses-compat'
   shutil.copy2(component,directory/candidate['component']);shutil.copy2(staged/'source.patch',directory/'source.patch')
   temporary=directory/'manifest.json.next';temporary.write_text(json.dumps(candidate,indent=2));os.replace(temporary,directory/'manifest.json')
  if managed and not replacement:
   plugin=next(p for p in client.call('GET','/api/v1/vendor-plugins')['data'] if p['vendor_id']==managed['vendor_id'])
   assert plugin['status']=='ready' and plugin['version']==managed['version'],'New host cannot load the managed compatibility plugin; rolling back'
  if replacement:
   component=staged_manifest.parent/replacement['component']
   preview=client.call('POST','/api/v1/vendor-plugins/import',component.read_bytes(),'application/wasm',timeout=600)['data']
   client.call('POST','/api/v1/vendor-plugins/confirm',{'preview_id':preview['id'],'allow_data_discard':False},timeout=600)
   plugin=next(p for p in client.call('GET','/api/v1/vendor-plugins')['data'] if p['vendor_id']==replacement['vendor_id'])
   assert plugin['status']=='ready' and plugin['version']==replacement['version'],'Rebuilt compatibility plugin could not be loaded'
   shutil.copy2(component,ROOT/'compat'/replacement['component'])
   shutil.copy2(staged_manifest.parent/'source.patch',ROOT/'compat/source.patch')
   temporary=compatibility.with_suffix('.json.next');temporary.write_text(json.dumps(replacement,indent=2));os.replace(temporary,compatibility)
  for component in components:
   preview=client.call('POST','/api/v1/vendor-plugins/import',component.read_bytes(),'application/wasm',timeout=600)['data']
   client.call('POST','/api/v1/vendor-plugins/confirm',{'preview_id':preview['id'],'allow_data_discard':False},timeout=600)
  statuses=client.call('GET','/api/v1/vendor-plugins')['data']
  assert all(p['status']=='ready' for p in statuses),'An installed plugin is unavailable after upgrade'
  run('python3','/root/cache-stravia-assets.py')
  ready()
 except BaseException:
  restore(backup,manifest);raise
 print(json.dumps({'upgraded':version,'backup':manifest,'preserved_compatibility_plugin':(replacement or managed)['version'] if managed else None}),flush=True)
parser=argparse.ArgumentParser();parser.add_argument('action',choices=['backup','upgrade','restore']);parser.add_argument('value',nargs='?');args=parser.parse_args()
if args.action=='backup':
 archive,manifest=snapshot('manual');run('rc-service','stravia','start');ready();print(json.dumps(manifest),flush=True)
elif args.action=='upgrade':upgrade(args.value or '')
else:
 path=pathlib.Path(args.value or '').resolve();assert path.is_relative_to(BACKUPS) and path.is_file()
 restore(path,json.loads(path.with_suffix('.json').read_text()))
