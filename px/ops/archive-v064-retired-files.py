"""Archive only unmatched inactive files/directories selected by the cleanup plan."""
import hashlib,json,os,pathlib,tarfile
os.umask(0o077);root=pathlib.Path('/root')
def digest(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  while block:=f.read(1024*1024):h.update(block)
 return h.hexdigest()
plan_path=root/'stravia-server-cleanup-plan.json';plan=json.loads(plan_path.read_text());decision=json.loads((root/'stravia-cleanup-transfer-decision.json').read_text())
assert digest(plan_path)==decision['plan_sha256']
allowed={item['path']:item for item in plan['files']}
sources=[]
for item in decision['files_to_archive']:
 assert item==allowed[item['path']]
 p=pathlib.Path(item['path']);assert p.is_file() and not p.is_symlink() and digest(p)==item['sha256'];sources.append(p)
dirs={item['path']:item for item in plan['directories']}
for value in decision['directories_to_archive']:
 item=dirs[value];p=pathlib.Path(value);assert p.is_dir() and not p.is_symlink()
 for entry in item['entries']:
  child=p/entry['relative_path']
  if 'sha256' in entry:assert child.is_file() and not child.is_symlink() and digest(child)==entry['sha256']
  else:assert child.is_symlink() and os.readlink(child)==entry['link']
 sources.append(p)
target=root/'stravia-retired-files-20261107.tar.gz'
with tarfile.open(target,'w:gz',compresslevel=1) as archive:
 for p in sources:archive.add(p,arcname=str(p).lstrip('/'))
target.chmod(0o600)
report={'archive':str(target),'bytes':target.stat().st_size,'sha256':digest(target),'plan_sha256':decision['plan_sha256'],'includes':[str(p) for p in sources]}
(root/'stravia-retired-files-archive.json').write_text(json.dumps(report,indent=2));print(json.dumps({k:report[k] for k in ['archive','bytes','sha256','plan_sha256']}),flush=True)
