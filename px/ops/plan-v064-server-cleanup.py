"""Inventory inactive Stravia/CPA files. No deletions; hot rollback stays protected."""
import hashlib,json,os,pathlib,sqlite3,stat
root=pathlib.Path('/root');platform=pathlib.Path('/opt/stravia')
def digest(p):
 h=hashlib.sha256()
 with pathlib.Path(p).open('rb') as f:
  while block:=f.read(1024*1024):h.update(block)
 return h.hexdigest()
current=json.loads((root/'stravia-request-model-current-backup.json').read_text())
previous=json.loads((root/'stravia-request-model-pre-change-backup.json').read_text())
assert current['mapping_survives_restart']
protected=[platform/'current',platform/'runtime',platform/'releases/v0.6.4/masub-responses',pathlib.Path(current['archive']),pathlib.Path(previous['archive'])]
for folder in [platform/'compat',platform/'responses-compat']:
 manifest=json.loads((folder/'manifest.json').read_text())
 protected.extend([folder/'manifest.json',folder/'source.patch',folder/manifest['component']])
protected=[p.resolve() for p in protected if p.exists()]
protected_inodes={(p.stat().st_dev,p.stat().st_ino) for p in protected if p.is_file()}
references=[]
for process in pathlib.Path('/proc').glob('[0-9]*'):
 try:
  references.extend(pathlib.Path(os.readlink(p)).resolve() for p in [process/'exe',process/'cwd',*list((process/'fd').iterdir())] if p.is_symlink() and os.readlink(p).startswith('/'))
  for line in (process/'maps').read_text().splitlines():
   value=line.split(maxsplit=5)
   if len(value)==6 and value[5].startswith('/'):references.append(pathlib.Path(value[5].removesuffix(' (deleted)')).resolve())
 except (OSError,ValueError):continue
for line in pathlib.Path('/proc/net/unix').read_text().splitlines():
 parts=line.split()
 if len(parts)>7 and parts[-1].startswith('/'):references.append(pathlib.Path(parts[-1]).resolve())
def in_use(path):
 p=path.resolve();s=p.stat()
 return (s.st_dev,s.st_ino) in protected_inodes or any(ref==p or ref.is_relative_to(p) for ref in protected+references)
files=[];directories=[];skipped=[]
patterns=['cpa-*.tar.gz','sub2api-to-cpa-*.tar.gz','legacy-before-stravia-*.tar.gz','stravia-ingress-before-*.tar.gz','stravia-api-paths-after-*.tar.gz','stravia-reports.tar.gz']
candidates=[p for pattern in patterns for p in root.glob(pattern)]
candidates.extend(pathlib.Path('/var/backups/stravia').glob('*.tar.gz'))
for p in sorted(set(candidates)):
 if p.is_symlink() or not p.is_file():continue
 if in_use(p):skipped.append(str(p));continue
 files.append({'path':str(p),'bytes':p.stat().st_size,'sha256':digest(p)})
 meta=p.with_suffix('.json')
 if meta.is_file():files.append({'path':str(meta),'bytes':meta.stat().st_size,'sha256':digest(meta)})
for p in [platform/'releases/v0.6.2',platform/'releases/v0.6.3',platform/'compat/staged/v0.6.2',platform/'compat/staged/v0.6.3',root/'stravia-v064-build-assets',pathlib.Path('/tmp/cliproxy-pluginhost')]:
 if not p.exists():continue
 assert not p.is_symlink()
 if in_use(p):skipped.append(str(p));continue
 entries=[]
 for child in sorted(p.rglob('*')):
  mode=child.lstat().st_mode
  if stat.S_ISREG(mode):entries.append({'relative_path':str(child.relative_to(p)),'bytes':child.stat().st_size,'sha256':digest(child)})
  elif child.is_symlink():entries.append({'relative_path':str(child.relative_to(p)),'link':os.readlink(child)})
 directories.append({'path':str(p),'entries':entries,'bytes':sum(item.get('bytes',0) for item in entries)})
report={'files':files,'directories':directories,'protected':[str(p) for p in protected],'skipped_in_use':skipped,'current_backup':current,'previous_backup':previous,'running_host_sha256':digest(platform/'current/stravia-server'),'nginx_sha256':{str(p):digest(p) for p in [pathlib.Path('/etc/nginx/http.d/stravia.conf'),pathlib.Path('/etc/nginx/stravia-proxy.inc')]}}
target=root/'stravia-server-cleanup-plan.json';target.write_text(json.dumps(report,indent=2))
print(json.dumps({'plan':str(target),'sha256':digest(target),'files':len(files),'directories':len(directories),'candidate_logical_bytes':sum(item['bytes'] for item in files+directories),'skipped_in_use':skipped}),flush=True)
