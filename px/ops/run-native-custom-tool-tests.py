"""Run native regressions with isolated temporary data, without Docker or mounts."""
import argparse,hashlib,json,os,pathlib,re,shutil,sqlite3,subprocess,tarfile
p=argparse.ArgumentParser();p.add_argument('--spec',type=pathlib.Path,default=pathlib.Path('/root/stravia-custom-tools-test-build.json'));args=p.parse_args()
spec=json.loads(args.spec.read_text());root=pathlib.Path('/root');store=pathlib.Path('/var/lib/stravia/artifacts')
def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as source:
  while chunk:=source.read(1024*1024):h.update(chunk)
 return h.hexdigest()
with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
 row=db.execute('select backend_key,state,storage_backend from artifacts where id=?',(spec['artifact_id'],)).fetchone()
assert row and row[1:]==('ready','internal')
package=(store/row[0]).resolve();assert package.is_relative_to(store) and digest(package)==spec['package_sha256']
directory=root/'stravia-custom-tool-tests-run';assert not directory.exists();directory.mkdir(mode=0o700)
counts={}
try:
 with tarfile.open(package,'r:gz') as archive:
  members=archive.getmembers();assert len(members)==1 and members[0].name=='stravia-core-tests' and members[0].isfile()
  binary=directory/'stravia-core-tests'
  with archive.extractfile(members[0]) as source,binary.open('wb') as out:shutil.copyfileobj(source,out)
 assert binary.stat().st_size==spec['binary_bytes'] and digest(binary)==spec['binary_sha256'];binary.chmod(0o700)
 temporary=directory/'tmp';temporary.mkdir(mode=0o700)
 loader=pathlib.Path('/opt/stravia/runtime/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2')
 for name,minimum in [('custom_tool',2),('request_models',9)]:
  command=[str(loader),'--library-path',str(loader.parent),str(binary),name,'--test-threads=1','--nocapture']
  result=subprocess.run(command,env={**os.environ,'TMPDIR':str(temporary),'TOKIO_WORKER_THREADS':'1','RAYON_NUM_THREADS':'1'},capture_output=True,text=True,timeout=600)
  log=root/('stravia-native-'+name+'-tests.log');log.write_text(result.stdout+'\n'+result.stderr);log.chmod(0o600)
  print(result.stdout[-5000:],flush=True);assert result.returncode==0,'Native regression failed'
  matched=re.search(r'test result: ok\. (\d+) passed; 0 failed;',result.stdout);assert matched and int(matched[1])>=minimum
  counts[name]=int(matched[1])
finally:
 assert directory.resolve()==root/'stravia-custom-tool-tests-run';shutil.rmtree(directory)
# Only expire the verified diagnostic artifact uploaded by this task.
with sqlite3.connect('/var/lib/stravia/db/gateway.db',timeout=30) as db:
 key=db.execute('select id from api_keys where name=?',('GPT-Private',)).fetchone()[0]
 assert db.execute('update artifacts set expires_at=0 where id=? and principal=? and state=? and storage_backend=?',(spec['artifact_id'],'api-key:'+key,'ready','internal')).rowcount==1
report={'passed':True,'custom_tool_tests':counts['custom_tool'],'mapping_tests':counts['request_models'],'source_commit':spec['source_commit'],'binary_sha256':spec['binary_sha256'],'isolated_temporary_data':True,'production_credentials_not_used_by_tests':True,'temporary_files_removed':True}
(root/'stravia-native-custom-tool-tests.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)
