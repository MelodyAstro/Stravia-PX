"""Replay the complete CC fixture through the native Anthropic HTTP surface."""
import copy,datetime,http.client,importlib.util,json,os,pathlib,subprocess,time
PUBLIC_HOST=os.environ['STRINOVA_PUBLIC_HOST']
assert PUBLIC_HOST and all(c.isalnum() or c in '.-' for c in PUBLIC_HOST)
spec=importlib.util.spec_from_file_location('client','/root/stravia-client.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);admin=m.Client().login()
access=json.loads(pathlib.Path('/root/stravia-access.json').read_text());key=next(g['api_key'] for g in access['groups'] if g['name']=='GA-Claude')
# The original CC capture has already been removed. Use a synthetic request
# with the same message/tool counts, output ceiling and adaptive thinking mode.
tools=[{'name':'echo_metadata','description':'Return diagnostic metadata.','input_schema':{'type':'object','properties':{'metadata':{'type':'object','additionalProperties':{'type':'string'}}},'required':['metadata'],'additionalProperties':False}}]
for index in range(37):tools.append({'name':'diagnostic_'+str(index),'description':'Unused diagnostic tool.','input_schema':{'type':'object','properties':{'value':{'type':'string'}},'additionalProperties':False}})
messages=[{'role':'user' if i%2==0 else 'assistant','content':'Reply only OK.' if i==6 else 'Diagnostic context.' if i%2==0 else 'Context noted.'} for i in range(7)]
original={'model':'claude-sonnet-4-6','system':'You are an Anthropic assistant performing a Claude Code compatibility diagnostic.','messages':messages,'tools':tools,'max_tokens':32000,'stream':True,'thinking':{'type':'adaptive'}}
results=[]
def generate(label,payload,public=False):
 start=time.monotonic();conn=http.client.HTTPSConnection(PUBLIC_HOST,timeout=180) if public else http.client.HTTPConnection('127.0.0.1',23471,timeout=180)
 path='/reviews/v1/messages' if public else '/v1/messages'
 conn.request('POST',path,json.dumps(payload).encode(),{'Content-Type':'application/json','x-api-key':key,'anthropic-version':'2023-06-01'})
 response=conn.getresponse();events=[]
 if response.status==200:
  for line in response:
   if not line.startswith(b'data:'):continue
   try:events.append(json.loads(line[5:].strip()))
   except ValueError:pass
 else:
  raw=response.read();error_path=pathlib.Path('/root/stravia-cc-compat-error-'+label+'.bin');error_path.write_bytes(raw);error_path.chmod(0o600)
 conn.close()
 item={'name':label,'status':response.status,'total_ms':round((time.monotonic()-start)*1000),'complete':any(e.get('type')=='message_stop' for e in events),'stream_error':any(e.get('type')=='error' for e in events),'tool_use':any(e.get('type')=='content_block_start' and e.get('content_block',{}).get('type')=='tool_use' for e in events),'stop_reason':next((e.get('delta',{}).get('stop_reason') for e in events if e.get('type')=='message_delta'),None)}
 results.append(item);print(json.dumps(item),flush=True);return item,events
def toggle(value):subprocess.run(['/usr/local/sbin/stravia-cc-compat',value],check=True)
def passed(item):return item['status']==200 and item['complete'] and not item['stream_error']
try:
 toggle('on')
 for model in ['claude-sonnet-4-6','claude-opus-4-6-thinking']:
  payload=copy.deepcopy(original);payload['model']=model
  item,_=generate('full-cc-switch-on-'+model,payload,True);assert passed(item),'Full CC replay failed'
 # Public tool-result round trip with an open dictionary. No user tools execute.
 tool={'name':'echo_metadata','description':'Return the provided test metadata.','input_schema':{'type':'object','properties':{'metadata':{'type':'object','additionalProperties':{'type':'string'}}},'required':['metadata'],'additionalProperties':False}}
 payload={'model':'claude-sonnet-4-6','max_tokens':8192,'stream':True,'system':'You are an Anthropic assistant. Call echo_metadata with metadata status=ok, then return OK after its result.','messages':[{'role':'user','content':'Run the echo_metadata test once.'}],'tools':[tool]}
 item,events=generate('dictionary-tool-call',payload,True);assert passed(item) and item['tool_use'],'Expected dictionary tool call'
 blocks={};tool_id=None
 for event in events:
  if event.get('type')=='content_block_start':blocks[event['index']]={'block':event['content_block'],'json':''}
  elif event.get('type')=='content_block_delta' and event['index'] in blocks:
   delta=event.get('delta',{});entry=blocks[event['index']]
   if delta.get('type')=='input_json_delta':entry['json']+=delta.get('partial_json','')
   elif delta.get('type')=='thinking_delta':entry['block']['thinking']=entry['block'].get('thinking','')+delta.get('thinking','')
   elif delta.get('type')=='signature_delta':entry['block']['signature']=entry['block'].get('signature','')+delta.get('signature','')
   elif delta.get('type')=='text_delta':entry['block']['text']=entry['block'].get('text','')+delta.get('text','')
 content=[]
 for index in sorted(blocks):
  entry=blocks[index];block=entry['block']
  if block['type']=='tool_use':
   block['input']=json.loads(entry['json'] or '{}');tool_id=block['id'];assert block['name']=='echo_metadata' and block['input'].get('metadata',{}).get('status')=='ok'
  content.append(block)
 payload['messages'] += [{'role':'assistant','content':content},{'role':'user','content':[{'type':'tool_result','tool_use_id':tool_id,'content':'{"status":"ok"}'}]}]
 item,_=generate('dictionary-tool-result',payload,True);assert passed(item) and item['stop_reason']=='end_turn','Expected successful tool result'
finally:toggle('on')
debug=admin.call('GET','/api/v1/observations/debug')['data'];assert not debug['enabled']
result={'time_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'results':results,'passed':all(passed(i) for i in results),'final_enabled':True,'debug_enabled':False,'full_cc_messages':len(original['messages']),'full_cc_tools':len(original['tools']),'full_cc_max_tokens':original['max_tokens'],'fixture_kind':'synthetic_cc_shape'}
pathlib.Path('/root/stravia-request-model-cc-verification.json').write_text(json.dumps(result,indent=2))
print(json.dumps({k:v for k,v in result.items() if k!='results'}),flush=True)
