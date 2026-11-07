const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),cp=require('node:child_process');
if(!process.argv[2])throw Error('Usage: node upload-test-parts.cjs <private-directory>');
const root=path.resolve(process.argv[2]);
const filename=path.join(root,'stravia-request-model-tests.tar.gz');
const specPath=path.join(root,'stravia-request-model-test-build.json');
const spec=JSON.parse(fs.readFileSync(specPath));
const transfer=spec.package_sha256.slice(0,12);
const input=fs.openSync(filename,'r'),parts=[];
try {
 for(let offset=0,number=1;offset<spec.package_bytes;offset+=64*1024*1024,number++){
  const size=Math.min(64*1024*1024,spec.package_bytes-offset),bytes=Buffer.alloc(size);
  if(fs.readSync(input,bytes,0,size,offset)!==size)throw Error('Incomplete local test package');
  const partFile=path.join(root,`request-model-tests-${transfer}-part-${number}.bin`);
  const receiptFile=path.join(root,`request-model-tests-${transfer}-part-${number}.json`);
  const sha=crypto.createHash('sha256').update(bytes).digest('hex');fs.writeFileSync(partFile,bytes);
  cp.execFileSync(process.execPath,[path.join(__dirname,'upload-build-artifact.cjs'),partFile,receiptFile,path.join(root,'stravia-access.json')],{stdio:'inherit'});
  const receipt=JSON.parse(fs.readFileSync(receiptFile));
  if(receipt.sha256!==sha||receipt.completed?.size!==size)throw Error('Test part receipt mismatch');
  parts.push({artifact_id:receipt.completed.path.replace('stravia://artifacts/',''),sha256:sha,bytes:size});
 }
} finally {fs.closeSync(input);}
delete spec.artifact_id;spec.artifact_parts=parts;spec.compressed_test_mount=true;
fs.writeFileSync(specPath,JSON.stringify(spec,null,2));
console.log(JSON.stringify({test_parts:parts.length,package_bytes:spec.package_bytes,package_sha256:spec.package_sha256}));
