"""Run native regressions from a read-only compressed image with isolated data.

Requires squashfs-tools-ng and kernel SquashFS support only for this test run.
The production executable remains an ordinary native GNU binary.
"""
import hashlib,json,os,pathlib,re,shutil,sqlite3,subprocess
root=pathlib.Path('/root');spec=json.loads((root/'stravia-request-model-test-build.json').read_text())
store=pathlib.Path('/var/lib/stravia/artifacts');parts=[]
with sqlite3.connect('file:/var/lib/stravia/db/gateway.db?mode=ro',uri=True) as db:
 for part in spec['artifact_parts']:
  row=db.execute('SELECT backend_key,state,storage_backend,size FROM artifacts WHERE id=?',(part['artifact_id'],)).fetchone()
  assert row and row[1:]==('ready','internal',part['bytes'])
  package=(store/row[0]).resolve();assert package.is_relative_to(store) and package.is_file()
  parts.append((package,part))
assert sum(part['bytes'] for _,part in parts)==spec['package_bytes']
directory=root/'stravia-request-model-tests-run';directory.mkdir(mode=0o700,exist_ok=True)
assert directory.resolve()==pathlib.Path('/root/stravia-request-model-tests-run')
mount=directory/'mounted';mount.mkdir(mode=0o700,exist_ok=True);assert not mount.is_mount()
image=directory/'tests.squashfs';build_log=directory/'image-build.log';mounted=False
try:
 with build_log.open('wb') as log:
  process=subprocess.Popen(['tar2sqfs','--force','--quiet','--no-skip','--no-xattr','--compressor','zstd','--num-jobs','1','--queue-backlog','2',str(image)],stdin=subprocess.PIPE,stdout=log,stderr=subprocess.STDOUT)
  combined=hashlib.sha256()
  try:
   for package,part in parts:
    checksum=hashlib.sha256();size=0
    with package.open('rb') as source:
     while block:=source.read(1024*1024):
      checksum.update(block);combined.update(block);size+=len(block);process.stdin.write(block)
    assert size==part['bytes'] and checksum.hexdigest()==part['sha256']
   assert combined.hexdigest()==spec['package_sha256']
   process.stdin.close()
   assert process.wait(timeout=600)==0,'Compressed image build failed'
  finally:
   if process.poll() is None:process.kill();process.wait()
 print(json.dumps({'compressed_test_image_bytes':image.stat().st_size,'binary_logical_bytes':spec['binary_bytes']}),flush=True)
 subprocess.run(['mount','-t','squashfs','-o','loop,ro,nodev,nosuid',str(image),str(mount)],check=True);mounted=True
 binary=mount/'stravia-core-tests';assert not binary.is_symlink() and binary.resolve().is_relative_to(mount.resolve()) and list(mount.iterdir())==[binary]
 checksum=hashlib.sha256()
 with binary.open('rb') as source:
  while block:=source.read(1024*1024):checksum.update(block)
 assert binary.stat().st_size==spec['binary_bytes'] and checksum.hexdigest()==spec['binary_sha256']
 temporary=directory/'tmp';temporary.mkdir(mode=0o700,exist_ok=True)
 loader=pathlib.Path('/opt/stravia/runtime/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2')
 command=[str(loader),'--library-path',str(loader.parent),str(binary),'request_models','--test-threads=1','--nocapture']
 result=subprocess.run(command,env={**os.environ,'TMPDIR':str(temporary),'TOKIO_WORKER_THREADS':'1','RAYON_NUM_THREADS':'1'},capture_output=True,text=True,timeout=600)
 log=root/'stravia-native-request-model-tests.log';log.write_text(result.stdout+'\n'+result.stderr);log.chmod(0o600)
 print(result.stdout[-5000:],flush=True)
 assert result.returncode==0,'Native tests failed; diagnostics retained in root-only log'
 match=re.search(r'test result: ok\. (\d+) passed; 0 failed;',result.stdout);assert match and int(match[1])>=9
finally:
 if mounted:subprocess.run(['umount',str(mount)],check=True)
 if build_log.is_file():shutil.copy2(build_log,root/'stravia-native-request-model-image.log')
 assert not mount.is_mount();shutil.rmtree(directory)
with sqlite3.connect('/var/lib/stravia/db/gateway.db',timeout=30) as db:
 key=db.execute('SELECT id FROM api_keys WHERE name=?',('GPT-Private',)).fetchone()[0]
 for _,part in parts:
  assert db.execute('UPDATE artifacts SET expires_at=0 WHERE id=? AND principal=? AND state=? AND storage_backend=?',(part['artifact_id'],'api-key:'+key,'ready','internal')).rowcount==1
report={'passed':True,'native_linux_tests':int(match[1]),'binary_sha256':spec['binary_sha256'],'isolated_temporary_data':True,'production_keys_not_used':True,'read_only_compressed_test_mount':True,'temporary_mount_removed':True}
(root/'stravia-native-request-model-tests.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)
