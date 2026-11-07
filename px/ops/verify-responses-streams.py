"""Verify complete Responses HTTP/WS streams without logging conversation data."""
import argparse,base64,copy,hashlib,http.client,json,pathlib,socket,ssl,struct,time
p=argparse.ArgumentParser(description=__doc__)
p.add_argument("--host",required=True,help="Public HTTPS hostname, without a path")
p.add_argument("--model",default="private/gpt-6-luna")
p.add_argument("--access-file",type=pathlib.Path,default=pathlib.Path("/root/stravia-access.json"))
p.add_argument("--fixture",type=pathlib.Path,help="Optional private Codex request; never committed or printed")
p.add_argument("--report",type=pathlib.Path,default=pathlib.Path("/root/stravia-responses-verification.json"))
p.add_argument("--group",default="GPT-Private",help="Existing isolated API-key group")
p.add_argument("--custom-tools",action="store_true",help="Verify free-form custom calls and result replay over HTTP and WS")
args=p.parse_args()
if not args.host or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-' for c in args.host):p.error("--host must contain only a hostname")
access=json.loads(args.access_file.read_text())
key=next(g['api_key'] for g in access['groups'] if g['name']==args.group and g['enabled'])
report=[]
def inspect(events,label,status,started):
 errors=[e for e in events if e.get('type') in ['error','response.failed'] or e.get('error')]
 completed=next((e['response'] for e in events if e.get('type')=='response.completed'),None)
 if errors or not completed:
  raise RuntimeError(label+': no successful completion; event types='+','.join(sorted({str(e.get('type')) for e in events})))
 item={'case':label,'status':status,'complete':True,'events':len(events),'output_types':sorted({i.get('type','') for i in completed.get('output',[])}),'total_ms':round((time.monotonic()-started)*1000)}
 report.append(item);print(json.dumps(item),flush=True)
 return completed
def http_request(payload,label,public=False):
 started=time.monotonic();host=args.host if public else '127.0.0.1'
 conn=http.client.HTTPSConnection(host,timeout=180) if public else http.client.HTTPConnection(host,23471,timeout=180)
 path='/comments/responses' if public else '/v1/responses'
 conn.request('POST',path,json.dumps(payload),{'Content-Type':'application/json','Authorization':'Bearer '+key})
 response=conn.getresponse();events=[]
 if response.status!=200:
  response.read();conn.close()
  raise RuntimeError(label+': HTTP '+str(response.status))
 for line in response:
  if not line.startswith(b'data:'):continue
  raw=line[5:].strip()
  if raw==b'[DONE]':continue
  events.append(json.loads(raw))
 conn.close();return inspect(events,label,response.status,started)
def websocket_request(payload,label,public=False):
 started=time.monotonic();host=args.host if public else '127.0.0.1'
 sock=socket.create_connection((host,443 if public else 23471),timeout=180)
 if public:sock=ssl.create_default_context().wrap_socket(sock,server_hostname=host)
 nonce=base64.b64encode(__import__('os').urandom(16)).decode();path='/comments/responses' if public else '/v1/responses'
 sock.sendall(('GET '+path+' HTTP/1.1\r\nHost: '+host+'\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: '+nonce+'\r\nSec-WebSocket-Version: 13\r\nOpenAI-Beta: responses_websockets=2026-02-06\r\nAuthorization: Bearer '+key+'\r\n\r\n').encode())
 buffer=bytearray()
 while b'\r\n\r\n' not in buffer:
  chunk=sock.recv(8192)
  if not chunk:raise RuntimeError('WebSocket closed during handshake')
  buffer.extend(chunk)
 head,remaining=bytes(buffer).split(b'\r\n\r\n',1);buffer=bytearray(remaining)
 assert head.startswith(b'HTTP/1.1 101 '),'WebSocket handshake rejected'
 expected=base64.b64encode(hashlib.sha1((nonce+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest())
 assert b'sec-websocket-accept: '+expected.lower() in head.lower()
 def read(size):
  while len(buffer)<size:
   chunk=sock.recv(max(8192,size-len(buffer)))
   if not chunk:raise RuntimeError('WebSocket closed before completion')
   buffer.extend(chunk)
  result=bytes(buffer[:size]);del buffer[:size];return result
 def send(opcode,body):
  mask=__import__('os').urandom(4);length=len(body)
  prefix=bytes([0x80|opcode,0x80|length]) if length<126 else bytes([0x80|opcode,0xfe])+struct.pack('!H',length) if length<65536 else bytes([0x80|opcode,0xff])+struct.pack('!Q',length)
  sock.sendall(prefix+mask+bytes(b^mask[i%4] for i,b in enumerate(body)))
 send(1,json.dumps(payload).encode());events=[];message=bytearray()
 try:
  while True:
   first,second=read(2);length=second&127
   if length==126:length=struct.unpack('!H',read(2))[0]
   elif length==127:length=struct.unpack('!Q',read(8))[0]
   assert not second&128 and length<32*1024*1024
   body=read(length);opcode=first&15
   if opcode==9:send(10,body);continue
   if opcode==10:continue
   if opcode==8:break
   assert opcode in [0,1],'Unexpected WS frame'
   message.extend(body)
   if not first&128:continue
   event=json.loads(message);message.clear();events.append(event)
   if event.get('type') in ['response.completed','response.failed','error']:break
 finally:sock.close()
 return inspect(events,label,101,started)
if args.custom_tools:
 for namespaced in [False,True]:
  tool={'type':'custom','name':'px_echo','description':'Echo tool. Call this tool once with exactly px-custom-ok as the complete raw input.','format':{'type':'grammar','syntax':'lark','definition':'start: "px-custom-ok"'}}
  declaration={'type':'namespace','name':'px_diag','description':'Diagnostic tools','tools':[tool]} if namespaced else tool
  for transport in ['http','websocket']:
   body={'model':args.model,'input':[{'role':'user','content':'Call px_echo once with raw input px-custom-ok. After receiving the tool result, reply with exactly OK.'}],'tools':[declaration],'tool_choice':'required','stream':True,'store':False,'reasoning':{'effort':'low'},'include':['reasoning.encrypted_content']}
   if namespaced:
    body['tools']=[];body['input'].insert(0,{'type':'additional_tools','role':'developer','tools':[declaration]})
   label=('namespaced-' if namespaced else '')+'custom-'+transport
   def invoke(payload,case):
    if transport=='http':return http_request(payload,case,public=True)
    frame=copy.deepcopy(payload);frame['type']='response.create';frame.pop('stream',None)
    return websocket_request(frame,case,public=True)
   first=invoke(body,label)
   calls=[i for i in first['output'] if i.get('type')=='custom_tool_call']
   assert len(calls)==1 and calls[0]['name']=='px_echo' and calls[0]['input']=='px-custom-ok','Custom tool identity/input mismatch: '+json.dumps([{'name':x.get('name'),'input_bytes':len(x.get('input','').encode()),'namespace':x.get('namespace')} for x in calls])
   assert not namespaced or calls[0].get('namespace')=='px_diag','Custom namespace changed'
   assert first['model']==args.model,'Mapped client model name changed'
   body['tool_choice']='none';body['input']+=first['output']+[{'type':'custom_tool_call_output','call_id':calls[0]['call_id'],'output':'px-custom-ok'}]
   final=invoke(body,label+'-result')
   assert any(i.get('type')=='message' for i in final['output']),'Tool result did not produce a message'
 output={'passed':True,'model':args.model,'fixture_kind':'synthetic_custom_tools','results':report}
 args.report.write_text(json.dumps(output,indent=2));print(json.dumps({'passed':True,'cases':len(report)}),flush=True)
 raise SystemExit(0)
header={'type':'additional_tools','role':'developer','tools':[{'type':'namespace','name':'px_diag','description':'A diagnostic echo namespace','tools':[{'type':'function','name':'echo','description':'Return the provided value unchanged','parameters':{'type':'object','properties':{'value':{'type':'string'}},'required':['value'],'additionalProperties':False},'strict':True}]}]}
original=json.loads(args.fixture.read_text()) if args.fixture else {'model':args.model,'input':[header,{'role':'user','content':'Reply with exactly OK without calling tools.'}],'tool_choice':'none','stream':True,'store':False,'reasoning':{'effort':'low'}}
original['model']=args.model
http_body=copy.deepcopy(original);http_body.pop('type',None)
http_request(http_body,'public-http',public=True)
original['type']='response.create';websocket_request(original,'public-websocket',public=True)
body={'model':args.model,'input':[header,{'role':'user','content':'Call px_diag.echo once with value stravia-responses-ok. After the tool result, reply with exactly OK.'}],'tool_choice':'required','stream':True,'store':False,'reasoning':{'effort':'low'},'include':['reasoning.encrypted_content']}
first=http_request(body,'namespaced-tool-public',public=True)
calls=[item for item in first.get('output',[]) if item.get('type')=='function_call']
assert len(calls)==1 and calls[0]['name']=='echo' and calls[0].get('namespace')=='px_diag','Namespaced function identity was not preserved'
assert json.loads(calls[0]['arguments'])=={'value':'stravia-responses-ok'}
body['tool_choice']='none';body['input']+=first['output']+[{'type':'function_call_output','call_id':calls[0]['call_id'],'output':'stravia-responses-ok'}]
final=http_request(body,'namespaced-tool-result-public',public=True)
assert any(item.get('type')=='message' for item in final['output'])
output={'passed':True,'model':args.model,'fixture_kind':'private_capture' if args.fixture else 'synthetic','original_input_items':len(original['input']),'namespace_preserved':True,'results':report}
args.report.write_text(json.dumps(output,indent=2));print(json.dumps({'passed':True}),flush=True)
