"""Exercise actual restore logic against temporary data and compatibility trees."""
import ast,contextlib,datetime,hashlib,io,json,pathlib,tarfile,tempfile,types
source=ast.parse(pathlib.Path('/root/stravia-maint-responses.py').read_text())
function=next(n for n in source.body if isinstance(n,ast.FunctionDef) and n.name=='restore')
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
with tempfile.TemporaryDirectory(prefix='stravia-restore-test-') as temporary:
 sandbox=pathlib.Path(temporary);root=sandbox/'opt/stravia';data=sandbox/'var/lib/stravia'
 release=root/'releases/v0.6.3';release.mkdir(parents=True);(release/'stravia-server').write_bytes(b'old host')
 data.mkdir(parents=True);(data/'plugin-state').write_text('new plugins')
 for name in ['compat','responses-compat']:
  directory=root/name;directory.mkdir();(directory/'manifest.json').write_text('new '+name)
 class RedirectedArchive:
  def __init__(self,p):self.archive=tarfile.open(p)
  def __enter__(self):return self
  def __exit__(self,*args):self.archive.close()
  def getmembers(self):return self.archive.getmembers()
  def extractall(self,destination,**kwargs):
   assert destination=='/'
   self.archive.extractall(sandbox,**kwargs)
 events=[]
 env={'ROOT':root,'DATA':data,'digest':digest,'pathlib':pathlib,'datetime':datetime,'subprocess':types.SimpleNamespace(run=lambda *a,**k:events.append('stop')),'tarfile':types.SimpleNamespace(open=RedirectedArchive),'os':types.SimpleNamespace(chown=lambda *a:None),'switch':lambda p:events.append(('switch',p)),'run':lambda *a:events.append('start'),'ready':lambda:events.append('ready'),'json':json}
 exec(compile(ast.Module(body=[function],type_ignores=[]),'<actual-maintenance-restore>','exec'),env)
 def archive(name,include_responses):
  package=sandbox/(name+'.tar.gz')
  with tarfile.open(package,'w:gz') as out:
   entries={'var/lib/stravia/plugin-state':b'old plugins','opt/stravia/compat/manifest.json':b'old compat'}
   if include_responses:entries['opt/stravia/responses-compat/manifest.json']=b'old responses'
   # Include the data directory itself, as real snapshots do.
   directory=tarfile.TarInfo('var/lib/stravia');directory.type=tarfile.DIRTYPE;out.addfile(directory)
   for path,content in entries.items():
    entry=tarfile.TarInfo(path);entry.size=len(content);out.addfile(entry,io.BytesIO(content))
  return package,{'sha256':digest(package),'release':str(release)}
 package,manifest=archive('with-responses',True)
 with contextlib.redirect_stdout(io.StringIO()):env['restore'](package,manifest)
 assert (data/'plugin-state').read_text()=='old plugins'
 assert (root/'compat/manifest.json').read_text()=='old compat'
 assert (root/'responses-compat/manifest.json').read_text()=='old responses'
 assert events[-3:]==[('switch',release),'start','ready']
 # Avoid timestamp collisions between two test restores without any sleep.
 for p in [data.with_name('stravia.failed-'+datetime.datetime.now().strftime('%Y%m%d%H%M%S')),*root.glob('*.retained-*')]:
  if p.exists():p.rename(p.with_name(p.name+'-test1'))
 package,manifest=archive('before-responses',False)
 with contextlib.redirect_stdout(io.StringIO()):env['restore'](package,manifest)
 assert not (root/'responses-compat').exists()
 assert (root/'compat/manifest.json').read_text()=='old compat' and (data/'plugin-state').read_text()=='old plugins'
 before=list(events);manifest['sha256']='0'*64
 try:env['restore'](package,manifest)
 except AssertionError as error:assert 'checksum mismatch' in str(error)
 else:raise AssertionError('Tampered snapshot must be rejected')
 assert events==before
print(json.dumps({'isolated_restore_tests':3,'restores_paired_metadata':True,'old_snapshot_removes_new_responses_manifest':True,'tamper_rejected_before_stop':True}))
