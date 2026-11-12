"""Local-only authenticated management helper; secrets never printed."""
import http.client,json,os,pathlib,urllib.parse

ORIGIN=os.environ['STRINOVA_ADMIN_ORIGIN'].rstrip('/')
origin=urllib.parse.urlsplit(ORIGIN)
if origin.scheme not in ['http','https'] or not origin.hostname or origin.username or origin.password or origin.path or origin.query or origin.fragment:
 raise ValueError('STRINOVA_ADMIN_ORIGIN must be a plain configured site origin')
ACCESS=pathlib.Path(os.environ.get('STRINOVA_ACCESS_FILE','/root/stravia-access.json'))

class Client:
 def __init__(self):self.cookies={};self.last_error=None
 def call(self,method,path,body=None,content_type='application/json',timeout=180):
  headers={'Host':origin.netloc,'X-Forwarded-Host':origin.netloc,'X-Forwarded-Proto':origin.scheme,'Origin':ORIGIN,'X-Stravia-CSRF':'1'}
  if self.cookies:headers['Cookie']='; '.join(k+'='+v for k,v in self.cookies.items())
  if body is not None:
   headers['Content-Type']=content_type
   if not isinstance(body,bytes):body=json.dumps(body).encode()
  conn=http.client.HTTPConnection('127.0.0.1',int(os.environ.get('STRINOVA_ADMIN_PORT','23471')),timeout=timeout)
  conn.request(method,path,body,headers)
  response=conn.getresponse();raw=response.read()
  for name,value in response.getheaders():
   if name.lower()=='set-cookie':
    pair=value.split(';',1)[0];key,val=pair.split('=',1);self.cookies[key]=val
  status=response.status;conn.close()
  try:result=json.loads(raw) if raw else None
  except ValueError:result={'non_json_bytes':len(raw)}
  if status>=400:
   self.last_error=result
   # Errors may contain credentials; report only a structured error code.
   code=result.get('error',{}).get('code') if isinstance(result,dict) and isinstance(result.get('error'),dict) else None
   raise RuntimeError(f'{method} {path}: HTTP {status}, code={code}')
  return result
 def login(self):
  secret=json.loads(ACCESS.read_text())
  self.call('POST','/api/v1/auth/login',{'username':secret['username'],'password':secret['password']})
  return self
