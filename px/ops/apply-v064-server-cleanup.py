"""Remove the explicit inactive plan only after verified local recovery copies."""
import hashlib,http.client,json,os,pathlib,shutil,sqlite3
root=pathlib.Path('/root');platform=pathlib.Path('/opt/stravia')
def digest(p):
 h=hashlib.sha256()
 with pathlib.Path(p).open('rb') as f:
  while block:=f.read(1024*1024):h.update(block)
 return h.hexdigest()
plan_path=root/'stravia-server-cleanup-plan.json';plan=json.loads(plan_path.read_text());receipt=json.loads((root/'stravia-cleanup-local-verified.json').read_text())
assert receipt['verified'] and digest(plan_path)==receipt['plan_sha256']
assert {item['path']:item['sha256'] for item in plan['files']}=={item['path']:item['sha256'] for item in receipt['files']}
assert set(receipt['directories'])=={item['path'] for item in plan['directories']}
assert digest(platform/'current/stravia-server')==plan['running_host_sha256']
assert digest(pathlib.Path(plan['previous_backup']['release'])/'stravia-server')=='b4ef0ad9071f8feb0568eaa89cb0f4656ceea39675e7f4fbc90757d81e406914'
for item in [plan['current_backup'],plan['previous_backup']]:assert digest(pathlib.Path(item['archive']))==item['sha256']
def fingerprint():
 with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
  data={}
  for table in ['api_keys','api_key_models','models','model_backends']:
   fields=[r[1] for r in db.execute('PRAGMA table_info('+table+')') if r[1] not in ['last_used_at','created_at','updated_at']]
   data[table]=db.execute('SELECT '+','.join('"'+field+'"' for field in fields)+' FROM '+table+' ORDER BY 1').fetchall()
  data['request_models']=db.execute('SELECT name,value FROM settings WHERE name LIKE ? ORDER BY name',('px_request_models:%',)).fetchall()
 return hashlib.sha256(json.dumps(data,sort_keys=True,default=str).encode()).hexdigest()
before=fingerprint();free_before=shutil.disk_usage('/').free;removed=[];skipped=[]
references=[]
for proc in pathlib.Path('/proc').glob('[0-9]*'):
 try:
  for p in [proc/'exe',proc/'cwd',*list((proc/'fd').iterdir())]:
   if p.is_symlink() and os.readlink(p).startswith('/'):references.append(pathlib.Path(os.readlink(p).removesuffix(' (deleted)')).resolve())
  for line in (proc/'maps').read_text().splitlines():
   values=line.split(maxsplit=5)
   if len(values)==6 and values[5].startswith('/'):references.append(pathlib.Path(values[5].removesuffix(' (deleted)')).resolve())
 except (OSError,ValueError):continue
for line in pathlib.Path('/proc/net/unix').read_text().splitlines():
 parts=line.split()
 if len(parts)>7 and parts[-1].startswith('/'):references.append(pathlib.Path(parts[-1]).resolve())
protected=[pathlib.Path(p) for p in plan['protected']]
protected_inodes={(p.stat().st_dev,p.stat().st_ino) for p in protected if p.is_file()}
def in_use(p):
 resolved=p.resolve();s=p.stat()
 return (s.st_dev,s.st_ino) in protected_inodes or any(ref==resolved or ref.is_relative_to(resolved) for ref in protected+references)
for item in plan['files']:
 p=pathlib.Path(item['path']);assert p.parent in [root,pathlib.Path('/var/backups/stravia')]
 if not p.exists():continue
 assert not p.is_symlink() and p.is_file() and digest(p)==item['sha256']
 if in_use(p):skipped.append(str(p));continue
 p.unlink();removed.append(str(p))
allowed_dirs={platform/'releases/v0.6.2',platform/'releases/v0.6.3',platform/'compat/staged/v0.6.2',platform/'compat/staged/v0.6.3',root/'stravia-v064-build-assets',pathlib.Path('/tmp/cliproxy-pluginhost')}
for item in plan['directories']:
 p=pathlib.Path(item['path']);assert p in allowed_dirs and p.resolve()==p and not p.is_symlink()
 if not p.exists():continue
 if in_use(p):skipped.append(str(p));continue
 actual={str(child.relative_to(p)) for child in p.rglob('*') if child.is_file() or child.is_symlink()}
 assert actual=={entry['relative_path'] for entry in item['entries']}
 for entry in item['entries']:
  child=p/entry['relative_path']
  if 'sha256' in entry:assert not child.is_symlink() and digest(child)==entry['sha256']
  else:assert child.is_symlink() and os.readlink(child)==entry['link']
 shutil.rmtree(p);removed.append(str(p))
# Remove obsolete root aliases only when their archived targets were retired.
retired={pathlib.Path(item['path']) for item in plan['files']}
for p in root.glob('stravia-before-*.tar.gz'):
 if p.is_symlink() and p.resolve() in retired:p.unlink();removed.append(str(p))
hashes={item['sha256'] for item in receipt['archives']}
staging=[*json.loads((root/'stravia-recovery-capsules.json').read_text())['archives'],json.loads((root/'stravia-retired-files-archive.json').read_text()),json.loads((root/'stravia-network-state-backup.json').read_text())]
for item in staging:
 p=pathlib.Path(item['archive']);assert p.parent==root and item['sha256'] in hashes
 if not p.exists():continue
 assert not p.is_symlink() and digest(p)==item['sha256']
 if any(ref==p for ref in references):skipped.append(str(p));continue
 p.unlink();removed.append(str(p))
assert before==fingerprint(),'Configuration changed during cleanup'
assert all(digest(pathlib.Path(p))==sha for p,sha in plan['nginx_sha256'].items())
assert digest(platform/'current/stravia-server')==plan['running_host_sha256']
for item in [plan['current_backup'],plan['previous_backup']]:assert digest(pathlib.Path(item['archive']))==item['sha256']
connection=http.client.HTTPConnection('127.0.0.1',23471,timeout=20);connection.request('GET','/readyz');response=connection.getresponse();response.read();connection.close();assert response.status==200
with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
 assert db.execute('PRAGMA quick_check').fetchone()[0]=='ok' and not db.execute('PRAGMA foreign_key_check').fetchall()
report={'cleaned':True,'local_recovery_verified':True,'removed':removed,'skipped_in_use':skipped,'free_before_bytes':free_before,'free_after_bytes':shutil.disk_usage('/').free,'reclaimed_bytes':shutil.disk_usage('/').free-free_before,'hot_backups_preserved':[plan['current_backup']['archive'],plan['previous_backup']['archive']],'current_and_previous_v064_programs_preserved':True,'keys_routes_and_mappings_unchanged':True,'nginx_unchanged':True,'ready':True,'sqlite_integrity':'ok'}
(root/'stravia-server-cleanup-result.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)
