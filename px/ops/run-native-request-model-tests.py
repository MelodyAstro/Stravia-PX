"""Run the fixed Linux test binary using the independent runtime and temp data."""
import hashlib,json,os,pathlib,re,shutil,sqlite3,subprocess,tarfile
root=pathlib.Path('/root');spec=json.loads((root/'stravia-request-model-test-build.json').read_text())
def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as stream:
  while data:=stream.read(1024*1024):h.update(data)
 return h.hexdigest()
with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
 row=db.execute('SELECT backend_key,state,storage_backend FROM artifacts WHERE id=?',(spec['artifact_id'],)).fetchone()
assert row and row[1:]==('ready','internal')
store=pathlib.Path('/var/lib/stravia/artifacts');package=(store/row[0]).resolve();assert package.is_relative_to(store) and digest(package)==spec['package_sha256']
directory=root/'stravia-request-model-tests-run';directory.mkdir(mode=0o700,exist_ok=True)
with tarfile.open(package) as archive:
 members=archive.getmembers();assert len(members)==1 and members[0].name=='stravia-core-tests' and members[0].isfile()
 archive.extractall(directory,filter='data')
binary=directory/'stravia-core-tests';binary.chmod(0o700);assert digest(binary)==spec['binary_sha256']
temporary=directory/'tmp';temporary.mkdir(mode=0o700,exist_ok=True)
loader=pathlib.Path('/opt/stravia/runtime/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2')
command=[str(loader),'--library-path',str(loader.parent),str(binary),'request_models','--test-threads=1','--nocapture']
result=subprocess.run(command,env={**os.environ,'TMPDIR':str(temporary),'TOKIO_WORKER_THREADS':'1','RAYON_NUM_THREADS':'1'},capture_output=True,text=True,timeout=600)
log=root/'stravia-native-request-model-tests.log';log.write_text(result.stdout+'\n'+result.stderr);log.chmod(0o600)
print(result.stdout[-5000:],flush=True)
assert result.returncode==0,'Native tests failed; diagnostics retained in root-only log'
match=re.search(r'test result: ok\. (\d+) passed; 0 failed;',result.stdout);assert match and int(match[1])>=8
report={'passed':True,'native_linux_tests':int(match[1]),'binary_sha256':spec['binary_sha256'],'isolated_temporary_data':True,'production_keys_not_used':True}
(root/'stravia-native-request-model-tests.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)
assert directory.resolve()==pathlib.Path('/root/stravia-request-model-tests-run');shutil.rmtree(directory)
