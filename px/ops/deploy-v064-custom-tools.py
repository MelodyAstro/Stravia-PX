"""Deploy a verified native custom-tool patch without changing routes or keys."""
import argparse,datetime,hashlib,importlib.util,json,os,pathlib,shutil,sqlite3,subprocess,tarfile,time,urllib.request
os.umask(0o077)
p=argparse.ArgumentParser();p.add_argument('--spec',type=pathlib.Path,default=pathlib.Path('/root/stravia-custom-tools-build.json'));args=p.parse_args()
root=pathlib.Path('/opt/stravia');private=pathlib.Path('/root');spec=json.loads(args.spec.read_text())
assert spec['upstream_tag']=='v0.6.4'
assert set(spec['features'])=={'positional_additional_tools','per_key_request_model_mapping_v1','native_custom_tools_v1'}
assert spec['codex_version']=='0.6.4+masub.responses.2'

def digest(path):
 h=hashlib.sha256()
 with pathlib.Path(path).open('rb') as f:
  while chunk:=f.read(1024*1024):h.update(chunk)
 return h.hexdigest()
def client():
 s=importlib.util.spec_from_file_location('client','/root/stravia-client.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m);return m.Client().login()
def ready():
 deadline=time.monotonic()+180
 while time.monotonic()<deadline:
  try:
   if urllib.request.urlopen('http://127.0.0.1:23471/readyz',timeout=3).status==200:return
  except OSError:pass
  time.sleep(1)
 raise RuntimeError('Native service readiness timed out')
def atomic(path,body):
 temporary=path.with_name(path.name+'.custom-tools-next');temporary.write_bytes(body);temporary.chmod(0o600);os.replace(temporary,path)
def fingerprint():
 result={}
 with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
  for table in ['api_keys','api_key_models','models','model_backends']:
   cols=[r[1] for r in db.execute('pragma table_info('+table+')') if r[1] not in ['last_used_at','created_at','updated_at']]
   rows=db.execute('select '+','.join('"'+x+'"' for x in cols)+' from '+table+' order by 1').fetchall();result[table]=hashlib.sha256(json.dumps(rows,sort_keys=True,default=str).encode()).hexdigest()
  result['request_models']=hashlib.sha256(json.dumps(db.execute("select name,value from settings where name like 'px_request_models:%' order by name").fetchall()).encode()).hexdigest()
 return result
previous=(root/'current').resolve();old=json.loads((root/'responses-compat/manifest.json').read_text())
assert digest(previous/'stravia-server')==spec['previous_host_sha256']==old['host_sha256']
assert set(old['features']).issubset(spec['features'])
tests=json.loads((private/'stravia-native-custom-tool-tests.json').read_text());assert tests['passed'] and tests['custom_tool_tests']>=2 and tests['mapping_tests']>=9
before=fingerprint();nginx={str(p):digest(p) for p in [pathlib.Path('/etc/nginx/http.d/stravia.conf'),pathlib.Path('/etc/nginx/stravia-proxy.inc')]}
ga=json.loads((root/'compat/manifest.json').read_text());assert digest(root/'compat'/ga['component'])==ga['sha256']
with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
 row=db.execute('select backend_key,state,storage_backend from artifacts where id=?',(spec['artifact_id'],)).fetchone()
assert row and row[1:]==('ready','internal')
store=pathlib.Path('/var/lib/stravia/artifacts');package=(store/row[0]).resolve();assert package.is_relative_to(store) and digest(package)==spec['package_sha256']
assert shutil.disk_usage(root).free>420*1024*1024
candidate=root/'releases/v0.6.4/masub-custom-tools-1';candidate.mkdir(mode=0o755,exist_ok=True);candidate.chmod(0o755)
expected={'stravia-server':spec['host_sha256'],'codex.wasm':spec['codex_sha256'],'base.wasm':spec['base_sha256'],'source.patch':spec['source_patch_sha256']}
with tarfile.open(package,'r:gz') as archive:
 members=archive.getmembers();assert {m.name for m in members}==set(expected) and all(m.isfile() for m in members)
 for member in members:
  assert member.size<200*1024*1024
  path=candidate/member.name
  with archive.extractfile(member) as src,path.open('wb') as out:shutil.copyfileobj(src,out)
  assert digest(path)==expected[member.name];path.chmod(0o755 if member.name=='stravia-server' else 0o600)
loader=root/'runtime/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2';version=subprocess.check_output([str(loader),'--library-path',str(loader.parent),str(candidate/'stravia-server'),'--version'],text=True).strip();assert '0.6.4' in version
result=subprocess.run(['/usr/local/sbin/stravia-maint','backup'],check=True,capture_output=True,text=True)
backup=json.loads(next(line for line in reversed(result.stdout.splitlines()) if line.startswith('{')))
try:
 subprocess.run(['rc-service','stravia','stop'],check=True)
 link=root/'current.next';link.unlink(missing_ok=True);link.symlink_to(candidate,target_is_directory=True);os.replace(link,root/'current')
 subprocess.run(['rc-service','stravia','start'],check=True);ready();c=client()
 preview=c.call('POST','/api/v1/vendor-plugins/import',(candidate/'codex.wasm').read_bytes(),'application/wasm',timeout=600);assert not preview.get('error')
 confirmed=c.call('POST','/api/v1/vendor-plugins/confirm',{'preview_id':preview['data']['id'],'allow_data_discard':False},timeout=600);assert not confirmed.get('error')
 plugins=c.call('GET','/api/v1/vendor-plugins')['data'];versions={x['vendor_id']:x['version'] for x in plugins};assert all(x['status']=='ready' for x in plugins)
 assert versions['openai-codex']==spec['codex_version'] and versions['antigravity']==ga['version']
 providers=c.call('GET','/api/v1/providers')['data'];g=next(x for x in providers if x['id']==ga['provider_id']);assert g['vendor_options']['claude_code_compat'] is True
 active=next(x for x in providers if x['name']=='GPT-Private');assert active['vendor_options']['websocket_url']=='wss://chatgpt.com/backend-api/codex/responses' and active['credential_status']=='ok'
 assert not c.call('GET','/api/v1/observations/debug')['data']['enabled']
 assert fingerprint()==before and all(digest(p)==sha for p,sha in nginx.items())
 component='stravia-vendor-openai-codex-v0.6.4-masub.responses.2.wasm'
 atomic(root/'responses-compat'/component,(candidate/'codex.wasm').read_bytes());atomic(root/'responses-compat/source.patch',(candidate/'source.patch').read_bytes())
 manifest={**old,'source_commit':spec['source_commit'],'source_base_commit':spec['source_base_commit'],'host_sha256':spec['host_sha256'],'component':component,'version':spec['codex_version'],'sha256':spec['codex_sha256'],'base_component_sha256':spec['base_sha256'],'source_patch_sha256':spec['source_patch_sha256'],'features':spec['features'],'built_at_utc':spec['built_at_utc']}
 atomic(root/'responses-compat/manifest.json',json.dumps(manifest,indent=2).encode())
 with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:assert db.execute('pragma quick_check').fetchone()[0]=='ok' and not db.execute('pragma foreign_key_check').fetchall()
 report={'deployed':True,'time_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_commit':spec['source_commit'],'host_sha256':spec['host_sha256'],'previous_release':str(previous),'current_release':str(candidate),'plugins':versions,'features':spec['features'],'backup':backup,'keys_routes_mappings_unchanged':True,'nginx_unchanged':True,'ga_switch_on':True,'upstream_websocket_url':active['vendor_options']['websocket_url'],'debug_off':True,'disk_free_bytes':shutil.disk_usage(root).free}
 atomic(private/'stravia-custom-tools-deployment.json',json.dumps(report,indent=2).encode());print(json.dumps(report),flush=True)
except BaseException:
 subprocess.run(['/usr/local/sbin/stravia-maint','restore',backup['archive']],check=True)
 raise
