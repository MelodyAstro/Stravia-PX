"""Exercise the real maintenance upgrade workflow with isolated fake releases."""
import ast,contextlib,hashlib,io,json,pathlib,shutil,sqlite3,sys,tarfile,tempfile,types
source=ast.parse(pathlib.Path(sys.argv[1] if len(sys.argv)>1 else '/usr/local/sbin/stravia-maint').read_text())
function=next(n for n in source.body if isinstance(n,ast.FunctionDef) and n.name=='upgrade')
with tempfile.TemporaryDirectory(prefix='stravia-upgrade-test-') as temporary:
 root=pathlib.Path(temporary);(root/'releases/v0.6.2').mkdir(parents=True);(root/'current').symlink_to(root/'releases/v0.6.2',target_is_directory=True)
 compat=root/'compat';compat.mkdir();component=compat/'managed.wasm';component.write_bytes(b'pinned compatibility component')
 digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
 managed={'component':component.name,'sha256':digest(component),'vendor_id':'antigravity','version':'0.6.2+masub.cc.1'}
 (compat/'manifest.json').write_text(json.dumps(managed))
 fixtures=root/'fixtures';fixtures.mkdir();server=fixtures/'stravia-server-v9.0.0-linux-x86_64-gnu.tar.gz'
 with tarfile.open(server,'w:gz') as package:
  data=b'fake staged host';entry=tarfile.TarInfo('server/stravia-server');entry.size=len(data);package.addfile(entry,io.BytesIO(data))
 plugin=fixtures/'stravia-vendor-claude-v9.0.0.wasm';plugin.write_bytes(b'official unrelated plugin')
 sums=fixtures/'SHA256SUMS';sums.write_text('\n'.join(digest(p)+'  '+p.name for p in [server,plugin]))
 release={'draft':False,'prerelease':False,'assets':[{'name':p.name,'browser_download_url':str(p)} for p in [server,plugin,sums]]}
 # There is deliberately no stock GA asset. A managed GA must remain installed.
 events=[];state={'incompatible':False,'lists':0}
 class Client:
  def login(self):return self
  def call(self,method,path,*args,**kwargs):
   if path=='/api/v1/vendor-plugins':
    state['lists']+=1
    plugins=[{'vendor_id':'antigravity','version':state.get('ga_version',managed['version']),'status':'error' if state['incompatible'] and state['lists']>1 else 'ready'},{'vendor_id':'claude','status':'ready'}]
    if state.get('responses'):plugins.append({'vendor_id':'openai-codex','version':state.get('responses_version','0.6.3+masub.responses.1'),'status':'ready'})
    return {'data':plugins}
   if path.endswith('/import'):
    rebuilt=args[0]==b'rebuilt compatibility component';responses=args[0]==b'rebuilt Responses component';kind='responses' if responses else 'managed' if rebuilt else 'claude';events.append('import-'+kind);return {'data':{'id':kind}}
   if path.endswith('/confirm'):
    if args[0]['preview_id']=='managed':state['ga_version']='9.0.0+masub.cc.1'
    if args[0]['preview_id']=='responses':state['responses_version']='9.0.0+masub.responses.1'
    events.append('confirm');return {}
   raise AssertionError(path)
 class Spec:
  loader=types.SimpleNamespace(exec_module=lambda m:setattr(m,'Client',Client))
 class DB:
  def __enter__(self):return self
  def __exit__(self,*args):pass
  def execute(self,*args):return types.SimpleNamespace(fetchone=lambda:((responses_active['sha256'] if args[1][0]=='openai-codex' else json.loads((compat/'manifest.json').read_text())['sha256']),))
 fake_import=types.SimpleNamespace(util=types.SimpleNamespace(spec_from_file_location=lambda *a:Spec(),module_from_spec=lambda *a:types.SimpleNamespace()))
 env={'ROOT':root,'re':__import__('re'),'urllib':types.SimpleNamespace(request=types.SimpleNamespace(urlopen=lambda *a,**k:io.StringIO(json.dumps(release)))),'json':json,'shutil':types.SimpleNamespace(disk_usage=lambda p:types.SimpleNamespace(free=1024**3)),'tarfile':tarfile,'pathlib':pathlib,'importlib':fake_import,'sqlite3':types.SimpleNamespace(connect=lambda *a,**k:DB()),'digest':digest,'download':lambda u,p,*a:shutil.copy2(u,p),'run':lambda *a:events.append(a[0]),'snapshot':lambda label:(root/'backup',{}),'switch':lambda p:events.append('switch'),'ready':lambda:None,'restore':lambda *a:events.append('restore')}
 exec(compile(ast.Module(body=[function],type_ignores=[]),'<actual-maintenance-upgrade>','exec'),env)
 with contextlib.redirect_stdout(io.StringIO()):env['upgrade']('v9.0.0')
 assert events.count('import-claude')==1 and 'restore' not in events and component.read_bytes()==b'pinned compatibility component'
 events.clear();state.update(incompatible=True,lists=0)
 try:
  with contextlib.redirect_stdout(io.StringIO()):env['upgrade']('v9.0.0')
 except AssertionError as error:assert 'cannot load' in str(error)
 else:raise AssertionError('Incompatible host must fail')
 assert 'restore' in events and 'import-claude' not in events
 events.clear();component.write_bytes(b'tampered')
 try:env['upgrade']('v9.0.0')
 except AssertionError as error:assert 'checksum mismatch' in str(error)
 else:raise AssertionError('Tampered component must fail')
 assert 'switch' not in events
 component.write_bytes(b'pinned compatibility component');events.clear();state.update(incompatible=False,lists=0)
 staged=compat/'staged/v9.0.0';staged.mkdir(parents=True)
 rebuilt=staged/'rebuilt.wasm';rebuilt.write_bytes(b'rebuilt compatibility component')
 patch=staged/'source.patch';patch.write_bytes(b'preserved compatibility patch')
 replacement={**managed,'version':'9.0.0+masub.cc.1','upstream_tag':'v9.0.0','component':rebuilt.name,'sha256':digest(rebuilt),'source_patch_sha256':digest(patch)}
 (staged/'manifest.json').write_text(json.dumps(replacement))
 env['shutil'].copy2=shutil.copy2;env['os']=__import__('os')
 with contextlib.redirect_stdout(io.StringIO()):env['upgrade']('v9.0.0')
 assert events.index('import-managed')<events.index('import-claude') and 'restore' not in events
 assert json.loads((compat/'manifest.json').read_text())==replacement
 assert (compat/rebuilt.name).read_bytes()==b'rebuilt compatibility component'
 # Exercise the same upgrade with a managed host+Codex pair. A release cannot
 # silently substitute its stock Codex component or discard positional tools.
 responses_dir=root/'responses-compat';responses_dir.mkdir()
 active_component=responses_dir/'codex.wasm';active_component.write_bytes(b'current Responses component')
 active_host=root/'releases/v0.6.2/stravia-server';active_host.write_bytes(b'current Responses host')
 responses_active={'vendor_id':'openai-codex','version':'0.6.3+masub.responses.1','component':active_component.name,'sha256':digest(active_component),'host_sha256':digest(active_host)}
 (responses_dir/'manifest.json').write_text(json.dumps(responses_active))
 helper=next(n for n in source.body if isinstance(n,ast.FunctionDef) and n.name=='responses_upgrade_plan')
 exec(compile(ast.Module(body=[helper],type_ignores=[]),'<actual-responses-preflight>','exec'),env)
 events.clear();state.update(responses=True,lists=0)
 try:env['upgrade']('v9.0.0')
 except RuntimeError as error:assert 'stage the target-version host' in str(error)
 else:raise AssertionError('Managed Responses upgrade must require a target-version pair')
 assert not events
 responses_staged=responses_dir/'staged/v9.0.0';responses_staged.mkdir(parents=True)
 candidate_host=responses_staged/'stravia-server';candidate_host.write_bytes(b'new Responses host')
 candidate_component=responses_staged/'codex.wasm';candidate_component.write_bytes(b'rebuilt Responses component')
 candidate_patch=responses_staged/'source.patch';candidate_patch.write_bytes(b'Responses compatibility source')
 candidate={**responses_active,'version':'9.0.0+masub.responses.1','upstream_tag':'v9.0.0','host_binary':candidate_host.name,'host_sha256':digest(candidate_host),'sha256':digest(candidate_component),'source_patch_sha256':digest(candidate_patch)}
 (responses_staged/'manifest.json').write_text(json.dumps(candidate))
 candidate_host.write_bytes(b'tampered')
 try:env['upgrade']('v9.0.0')
 except AssertionError as error:assert 'host checksum mismatch' in str(error)
 else:raise AssertionError('Tampered host must fail before switching')
 assert not events
 candidate_host.write_bytes(b'new Responses host')
 with contextlib.redirect_stdout(io.StringIO()):env['upgrade']('v9.0.0')
 assert events.index('import-responses')<events.index('import-claude')
 assert 'restore' not in events and events.count('import-claude')==1
 assert json.loads((responses_dir/'manifest.json').read_text())==candidate
 assert (root/'releases/v9.0.0/masub-responses/stravia-server').read_bytes()==b'new Responses host'
 assert candidate_host.stat().st_ino==(root/'releases/v9.0.0/masub-responses/stravia-server').stat().st_ino
 # The first install must use the staged pair even before an active Responses
 # manifest exists; it must not download/import the stock Codex component.
 (responses_dir/'manifest.json').unlink();events.clear();state.update(lists=0,responses_version='0.6.3')
 with contextlib.redirect_stdout(io.StringIO()):env['upgrade']('v9.0.0')
 assert events.count('import-responses')==1 and 'restore' not in events
 assert json.loads((responses_dir/'manifest.json').read_text())==candidate
print(json.dumps({'isolated_upgrade_tests':8,'preserves_managed_ga':True,'updates_other_plugins':True,'incompatible_host_rolls_back':True,'tamper_stops_before_switch':True,'rebuilt_compatibility_installed_atomically':True,'requires_staged_responses_pair':True,'preserves_rebuilt_responses_plugin':True,'first_responses_install_uses_staged_pair':True,'host_uses_verified_hardlink':True}))
