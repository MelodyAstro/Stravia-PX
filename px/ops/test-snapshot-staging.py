"""Verify actual snapshots retain active state and omit candidate caches."""
import ast,datetime,hashlib,json,pathlib,sys,tarfile,tempfile,types
source=ast.parse(pathlib.Path(sys.argv[1] if len(sys.argv)>1 else '/root/stravia-maint-responses.py').read_text())
functions=[n for n in source.body if isinstance(n,ast.FunctionDef) and n.name in ['snapshot_filter','snapshot']]
with tempfile.TemporaryDirectory(prefix='stravia-snapshot-test-') as temporary:
 sandbox=pathlib.Path(temporary);root=sandbox/'opt/stravia';data=sandbox/'var/lib/stravia';backups=sandbox/'var/backups/stravia'
 release=root/'releases/v0.6.3';release.mkdir(parents=True);(root/'current').symlink_to(release,target_is_directory=True)
 (data/'db').mkdir(parents=True);(data/'db/gateway.db').write_bytes(b'isolated database fixture')
 backups.mkdir(parents=True)
 for name in ['compat','responses-compat']:
  directory=root/name;directory.mkdir();(directory/'manifest.json').write_text('active manifest');(directory/'component.wasm').write_bytes(b'active component');(directory/'source.patch').write_text('active source')
  staged=directory/'staged/v9.0.0';staged.mkdir(parents=True);(staged/'stravia-server').write_bytes(b'candidate host');(staged/'candidate.wasm').write_bytes(b'candidate component')
 events=[]
 digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
 env={'ROOT':root,'DATA':data,'BACKUPS':backups,'datetime':datetime,'run':lambda *a:events.append(a),'tarfile':tarfile,'pathlib':types.SimpleNamespace(Path=lambda p:sandbox/p.lstrip('/')),'digest':digest,'json':json}
 exec(compile(ast.Module(body=functions,type_ignores=[]),'<actual-maintenance-snapshot>','exec'),env)
 archive,manifest=env['snapshot']('isolated')
 with tarfile.open(archive) as package:
  names=package.getnames()
  assert not any('/staged' in name for name in names)
  for name in ['compat','responses-compat']:
   for filename in ['manifest.json','component.wasm','source.patch']:
    assert 'opt/stravia/'+name+'/'+filename in names
  assert package.extractfile('var/lib/stravia/db/gateway.db').read()==b'isolated database fixture'
 assert manifest['sha256']==digest(archive) and manifest['release']==str(release)
 assert events==[('rc-service','stravia','stop')]
print(json.dumps({'isolated_snapshot_test':True,'active_data_and_patch_files_retained':True,'staged_candidates_excluded':True,'checksum_and_release_pointer_verified':True}))
