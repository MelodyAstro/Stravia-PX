"""Archive system configuration, bootstrap tools and the independent GNU runtime.

Contains secrets. Download only into the restricted local recovery directory.
Application data is backed up separately by stravia-maint with the service stopped.
"""
import datetime,hashlib,json,os,pathlib,re,subprocess,tarfile
os.umask(0o077)
root=pathlib.Path('/root');platform=pathlib.Path('/opt/stravia')
active=json.loads((platform/'responses-compat/manifest.json').read_text())
def digest(p):
 h=hashlib.sha256()
 with pathlib.Path(p).open('rb') as stream:
  while block:=stream.read(1024*1024):h.update(block)
 return h.hexdigest()
assert digest(platform/'current/stravia-server')==active['host_sha256']
stamp=datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
paths=[pathlib.Path(p) for p in ['/etc','/usr/local/sbin','/usr/local/libexec','/root/stravia-access.json','/root/stravia-client.py','/root/cache-stravia-assets.py','/root/.ssh/authorized_keys','/root/.acme.sh','/root/hypernet','/var/www/stravia','/usr/local/bin/hysteria','/usr/local/bin/xray','/usr/local/bin/cloudflared']]
for config in pathlib.Path('/etc/nginx').rglob('*'):
 if config.is_file() and config.suffix in ['.conf','.inc']:
  for value in re.findall(r'\bssl_certificate(?:_key)?\s+([^;\s]+)',config.read_text()):
   p=pathlib.Path(value.strip('\"\''))
   if p.is_file():paths.extend([p,p.resolve()])
cron=pathlib.Path('/etc/crontabs/root')
if cron.is_file():
 for value in re.findall(r'/(?:root|usr/local)/[A-Za-z0-9_./-]+',cron.read_text()):
  p=pathlib.Path(value)
  if p.is_file():paths.append(p)
selected=[]
for p in sorted(set(paths),key=lambda p:len(p.parts)):
 if not p.exists():continue
 if any(parent.is_dir() and p.is_relative_to(parent) for parent in selected):continue
 selected.append(p)
archives=[]
network_state=[pathlib.Path(p) for p in ['/var/lib/hypernet','/var/lib/cloudflared','/var/lib/unbound','/root/.cloudflared'] if pathlib.Path(p).exists()]
for kind,sources in [('server-config-and-tools',selected),('stravia-gnu-runtime',[platform/'runtime']),('network-state',network_state)]:
 if not sources:continue
 target=root/f'{kind}-{stamp}.tar.gz'
 with tarfile.open(target,'w:gz',compresslevel=1) as archive:
  for p in sources:archive.add(p,arcname=str(p).lstrip('/'))
 target.chmod(0o600)
 archives.append({'kind':kind,'archive':str(target),'bytes':target.stat().st_size,'sha256':digest(target),'includes':[str(p) for p in sources]})
report={'created_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'contains_credentials':True,'current_release':str((platform/'current').resolve()),'host_sha256':active['host_sha256'],'features':active['features'],'source_commit':active['source_commit'],'archives':archives,'packages':subprocess.check_output(['apk','info','-vv'],text=True).splitlines()}
(root/'stravia-recovery-capsules.json').write_text(json.dumps(report,indent=2))
print(json.dumps({'created':True,'archives':[{k:item[k] for k in ['kind','archive','bytes','sha256']} for item in archives]}),flush=True)
