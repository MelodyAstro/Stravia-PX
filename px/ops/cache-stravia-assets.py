"""Copy reachable immutable WebUI assets from the unmodified embedded server."""
import collections,gzip,hashlib,http.client,json,os,pathlib,re,urllib.parse
HOST=os.environ.get('STRINOVA_PUBLIC_HOST','localhost')
ROOT=pathlib.Path('/var/www/stravia')
ROOT.mkdir(parents=True,exist_ok=True,mode=0o755)
queue=collections.deque(['/']);seen=set();saved=[];ignored=[]
pattern=re.compile(r'''["'`]([^"'`\s<>]+\.(?:js|css|woff2?|ttf|otf|svg|png|jpe?g|webp|ico|wasm)(?:\?[^"'`\s]*)?)["'`]''')
while queue:
 path=queue.popleft()
 if path in seen:continue
 seen.add(path)
 assert len(seen)<5000,'Unexpected asset graph size'
 conn=http.client.HTTPConnection('127.0.0.1',23471,timeout=30)
 conn.request('GET',path,headers={'Host':HOST,'X-Forwarded-Host':HOST,'X-Forwarded-Proto':'https'})
 response=conn.getresponse();body=response.read();kind=response.getheader('Content-Type','');status=response.status;conn.close()
 assert status==200,(path,status)
 if path!='/':
  assert path.startswith('/_app/immutable/')
  if 'text/html' in kind:
   # Bundles also contain documentation/example strings and optional worker
   # defaults. The embedded server returns its SPA shell for these non-assets.
   ignored.append(path);continue
  target=(ROOT/path.lstrip('/')).resolve();assert target.is_relative_to(ROOT)
  target.parent.mkdir(parents=True,exist_ok=True)
  if target.exists():assert hashlib.sha256(target.read_bytes()).digest()==hashlib.sha256(body).digest(),'Immutable asset content changed'
  else:target.write_bytes(body);target.chmod(0o644)
  if path.endswith(('.js','.css','.svg','.wasm')):
   compressed=target.with_name(target.name+'.gz');compressed.write_bytes(gzip.compress(body,compresslevel=6,mtime=0));compressed.chmod(0o644)
  saved.append({'path':path,'sha256':hashlib.sha256(body).hexdigest(),'bytes':len(body)})
 if path=='/' or path.endswith(('.js','.css')):
  text=body.decode('utf-8')
  candidates=pattern.findall(text)
  if path=='/':candidates+=re.findall(r'import\("([^"]+)"\)',text)
  if path.endswith('.css'):candidates+=re.findall(r'url\(([^)]+)\)',text)
  for candidate in candidates:
   candidate=candidate.strip('"\'')
   if not candidate.startswith(('./','../','/_app/','_app/')) or '${' in candidate:continue
   resolved=urllib.parse.urlsplit(urllib.parse.urljoin(path,candidate)).path
   if resolved.startswith('/_app/immutable/') and resolved not in seen:queue.append(resolved)
assert len(saved)>5,'Failed to discover embedded WebUI assets'
manifest=pathlib.Path('/opt/stravia/current').resolve()/'webui-assets.json';manifest.write_text(json.dumps(saved,indent=2))
print(json.dumps({'immutable_assets':len(saved),'bytes':sum(x['bytes'] for x in saved),'non_asset_literals':ignored,'manifest':str(manifest)}),flush=True)
