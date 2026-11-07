"""Stage a verified same-version native feature build, snapshot, switch and verify."""
import datetime,gzip,hashlib,importlib.util,json,os,pathlib,shutil,sqlite3,subprocess,tarfile,time,urllib.request
os.umask(0o077)
root=pathlib.Path('/opt/stravia');private=pathlib.Path('/root')
spec=json.loads((private/'stravia-request-model-host-build.json').read_text())
assert spec['upstream_tag']=='v0.6.4' and spec['upstream_commit']=='a100ef42031073fbdd779e4809fb83cac3a8b956'
assert set(spec['features'])=={'positional_additional_tools','per_key_request_model_mapping_v1'}
assert spec['host_sha256']=='53be51b71ca1b4a5804262bf1b46c49903da3a72f7a3d72658ac0f6800b0ab2e','This one-time deployment requires the final response-name fix'
native_tests=json.loads((private/'stravia-native-request-model-tests.json').read_text())
assert native_tests['passed'] and native_tests['native_linux_tests']>=9,'Run the final native mapping regressions before deployment'
def digest(path):
 h=hashlib.sha256()
 with pathlib.Path(path).open('rb') as stream:
  while data:=stream.read(1024*1024):h.update(data)
 return h.hexdigest()
def atomic(path,body):
 p=pathlib.Path(path);temporary=p.with_name(p.name+'.request-model-next');temporary.write_bytes(body);temporary.chmod(0o600);os.replace(temporary,p)
def client():
 s=importlib.util.spec_from_file_location('client','/root/stravia-client.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m);return m.Client().login()
def wait_ready():
 deadline=time.monotonic()+180
 while time.monotonic()<deadline:
  try:
   if urllib.request.urlopen('http://127.0.0.1:23471/readyz',timeout=3).status==200:return
  except OSError:pass
  time.sleep(1)
 raise RuntimeError('Native service readiness timed out')
def fingerprint():
 with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
  result={}
  for table in ['api_keys','api_key_models','models','model_backends']:
   kept=[r[1] for r in db.execute('PRAGMA table_info('+table+')') if r[1] not in ['last_used_at','created_at','updated_at']]
   rows=db.execute('SELECT '+','.join('"'+k+'"' for k in kept)+' FROM '+table+' ORDER BY 1').fetchall()
   result[table]=hashlib.sha256(json.dumps(rows,sort_keys=True,default=str).encode()).hexdigest()
  return result
before=fingerprint();nginx={str(p):digest(p) for p in [pathlib.Path('/etc/nginx/http.d/stravia.conf'),pathlib.Path('/etc/nginx/stravia-proxy.inc')]}
previous=(root/'current').resolve();assert previous==root/'releases/v0.6.4/masub-responses'
old_manifest=json.loads((root/'responses-compat/manifest.json').read_text())
assert digest(previous/'stravia-server')==old_manifest['host_sha256']
assert digest(private/'stravia-maint-request-models.py')==spec['maint_sha256']
patch=private/'stravia-request-model-source.patch';assert digest(patch)==spec['source_patch_sha256']
with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
 row=db.execute('SELECT backend_key,state,storage_backend FROM artifacts WHERE id=?',(spec['host_artifact_id'],)).fetchone()
assert row and row[1:]==('ready','internal')
store=pathlib.Path('/var/lib/stravia/artifacts');package=(store/row[0]).resolve()
assert package.is_relative_to(store) and digest(package)==spec['package_sha256']
assert shutil.disk_usage(root).free>450*1024*1024,'Insufficient staging/backup space'
candidate=root/'releases/v0.6.4/masub-request-models-1';candidate.mkdir(mode=0o755,exist_ok=True);candidate.chmod(0o755)
static=pathlib.Path('/var/www/stravia/_app/immutable');asset_count=0;found_binary=False
with tarfile.open(package,'r|gz') as archive:
 for member in archive:
  name=pathlib.PurePosixPath(member.name)
  assert not name.is_absolute() and '..' not in name.parts and (member.isfile() or member.isdir())
  if member.name=='stravia-server':
   assert member.isfile() and member.size==spec['host_bytes'];target=candidate/'stravia-server'
   with archive.extractfile(member) as source,target.open('wb') as out:shutil.copyfileobj(source,out)
   target.chmod(0o755);found_binary=True
  else:
   assert name.parts[0]=='immutable'
   if member.isdir():continue
   relative=pathlib.Path(*name.parts[1:]);target=static/relative
   assert target.resolve().is_relative_to(static.resolve())
   body=archive.extractfile(member).read();target.parent.mkdir(parents=True,exist_ok=True,mode=0o755)
   parent=target.parent
   while parent.is_relative_to(static):
    parent.chmod(0o755)
    if parent==static:break
    parent=parent.parent
   if target.exists():assert hashlib.sha256(target.read_bytes()).digest()==hashlib.sha256(body).digest(),'Immutable asset collision'
   else:target.write_bytes(body);target.chmod(0o644)
   if target.suffix in ['.js','.css','.svg','.wasm']:
    compressed=target.with_name(target.name+'.gz');compressed.write_bytes(gzip.compress(body,compresslevel=6,mtime=0));compressed.chmod(0o644)
   asset_count+=1
assert found_binary and asset_count==spec['immutable_asset_count'] and digest(candidate/'stravia-server')==spec['host_sha256']
loader=root/'runtime/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2'
version=subprocess.run([str(loader),'--library-path',str(loader.parent),str(candidate/'stravia-server'),'--version'],capture_output=True,text=True,check=True,timeout=20).stdout.strip();assert '0.6.4' in version
manifest={**old_manifest,**{k:spec[k] for k in ['upstream_tag','upstream_commit','source_base_commit','source_commit','source_patch_sha256','host_sha256','features']}}
manifest['built_at_utc']=spec['built_at_utc']
staged=root/'responses-compat/staged/v0.6.4';staged.mkdir(parents=True,exist_ok=True)
temporary=staged/'stravia-server.request-model-next';temporary.unlink(missing_ok=True);os.link(candidate/'stravia-server',temporary);os.replace(temporary,staged/'stravia-server')
atomic(staged/'source.patch',patch.read_bytes());atomic(staged/'manifest.json',json.dumps(manifest,indent=2).encode())
shutil.copy2(private/'stravia-maint-request-models.py','/usr/local/sbin/stravia-maint');pathlib.Path('/usr/local/sbin/stravia-maint').chmod(0o700)
# This exact, verified upload is only a deployment transport. Use the same
# expiry marker as ArtifactStore.delete_ready; Stravia performs its normal GC
# on restart. The staged executable and local package remain available.
with sqlite3.connect('/var/lib/stravia/db/gateway.db',timeout=30) as db:
 key=db.execute('SELECT id FROM api_keys WHERE name=?',('GPT-Private',)).fetchone()[0]
 assert db.execute('UPDATE artifacts SET expires_at=0 WHERE id=? AND principal=? AND state=? AND storage_backend=?',(spec['host_artifact_id'],'api-key:'+key,'ready','internal')).rowcount==1
backup_process=subprocess.run(['/usr/local/sbin/stravia-maint','backup'],capture_output=True,text=True,check=True,timeout=300)
backup=next(json.loads(line) for line in backup_process.stdout.splitlines() if line.startswith('{') and 'archive' in json.loads(line))
(private/'stravia-request-model-pre-change-backup.json').write_text(json.dumps(backup,indent=2))
print(json.dumps({'staged':True,'backup':backup,'immutable_assets_ready':asset_count}),flush=True)
try:
 subprocess.run(['rc-service','stravia','stop'],check=True)
 link=root/'current.request-model-next';link.unlink(missing_ok=True);link.symlink_to(candidate,target_is_directory=True);os.replace(link,root/'current')
 atomic(root/'responses-compat/source.patch',patch.read_bytes());atomic(root/'responses-compat/manifest.json',json.dumps(manifest,indent=2).encode())
 subprocess.run(['rc-service','stravia','start'],check=True);wait_ready();c=client()
 plugins=c.call('GET','/api/v1/vendor-plugins')['data'];assert len(plugins)==4 and all(p['status']=='ready' for p in plugins)
 versions={p['vendor_id']:p['version'] for p in plugins};assert versions['antigravity']=='0.6.4+masub.cc.1' and versions['openai-codex']=='0.6.4+masub.responses.1'
 assert not c.call('GET','/api/v1/observations/debug')['data']['enabled']
 ga=json.loads((root/'compat/manifest.json').read_text());provider=next(p for p in c.call('GET','/api/v1/providers')['data'] if p['id']==ga['provider_id']);assert provider['vendor_options']['claude_code_compat'] is True
 models=c.call('GET','/api/v1/models')['data'];keys=c.call('GET','/api/v1/api-keys')['data'];configured=[]
 for key in keys:
  if key['name'] not in ['GPT-Private','All'] or not key['is_enabled']:continue
  path='/api/v1/api-keys/'+key['id']+'/request-models';view=c.call('GET',path)['data']
  assert view['config']=={'enabled':False,'mappings':[]},'Unexpected preexisting mappings; preserve and review them before applying defaults'
  mappings=[{'request_model':model['model_id'].removeprefix('private/'),'target_model_id':model['id']} for model in models if model['model_id'].startswith('private/') and (not key['model_ids'] or model['id'] in key['model_ids'])]
  assert len(mappings)==8
  saved=c.call('PUT',path,{'enabled':True,'mappings':mappings})['data'];assert saved['config']=={'enabled':True,'mappings':mappings}
  configured.append({'key_name':key['name'],'model_mappings':len(mappings)})
 assert len(configured)==2 and fingerprint()==before and all(digest(p)==expected for p,expected in nginx.items())
 with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
  assert db.execute('PRAGMA quick_check').fetchone()[0]=='ok' and not db.execute('PRAGMA foreign_key_check').fetchall()
 assert digest(root/'current/stravia-server')==spec['host_sha256']
 report={'deployed':True,'time_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'version':version,'source_commit':spec['source_commit'],'current_release':str(candidate),'previous_release':str(previous),'host_sha256':spec['host_sha256'],'features':spec['features'],'defaults':configured,'ga_switch_on':True,'debug_off':True,'keys_and_routes_unchanged':True,'nginx_unchanged':True,'sqlite_integrity':'ok','plugins':versions,'backup':backup,'immutable_assets_ready':asset_count,'disk_free_bytes':shutil.disk_usage(root).free}
 (private/'stravia-request-model-deployment.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)
except BaseException:
 subprocess.run(['/usr/local/sbin/stravia-maint','restore',backup['archive']],check=True)
 raise
