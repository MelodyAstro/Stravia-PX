"""Verify bare model aliases stay isolated between active Team and Private Keys."""
import http.client,importlib.util,json,pathlib,sqlite3
s=importlib.util.spec_from_file_location('client','/root/stravia-client.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m);c=m.Client().login()
keys=c.call('GET','/api/v1/api-keys')['data'];routes=c.call('GET','/api/v1/models')['data'];providers=c.call('GET','/api/v1/providers')['data'];access=json.loads(m.ACCESS.read_text())
def request(method,path,group,body=None):
 key=next(g['api_key'] for g in access['groups'] if g['name']==group and g['enabled']);conn=http.client.HTTPConnection('127.0.0.1',23471,timeout=20)
 conn.request(method,path,None if body is None else json.dumps(body),{'Authorization':'Bearer '+key,'Content-Type':'application/json'})
 response=conn.getresponse();data=response.read();status=response.status;conn.close();return status,json.loads(data)
summary=[]
for group,prefix in [('GPT-Private','private/'),('GPT-Team','team/')]:
 key=next(k for k in keys if k['name']==group);assert key['is_enabled'] and len(key['model_ids'])==8
 own=[r for r in routes if r['id'] in key['model_ids']];assert len(own)==8 and all(r['model_id'].startswith(prefix) for r in own)
 view=c.call('GET','/api/v1/api-keys/'+key['id']+'/request-models')['data'];assert view['config']['enabled'] and len(view['config']['mappings'])==8
 status,models=request('GET','/v1/models',group);assert status==200;names=sorted(x['id'] for x in models['data']);assert names==sorted(r['model_id'].removeprefix(prefix) for r in own)
 for route in own:
  assert all(next(p for p in providers if p['id']==target['provider_id'])['name'].startswith(group) for target in route['targets'])
 other='team/' if prefix=='private/' else 'private/'
 status,_=request('POST','/v1/responses',group,{'model':other+'gpt-6.1-sol','input':'Isolated authorization check','stream':True});assert status==403,'Cross-account model access was not denied'
 summary.append({'group':group,'listed_models':names,'cross_account_status':status,'mapping_targets_authorized':True})
report={'passed':True,'groups':summary,'group_keys_rotated':False};pathlib.Path('/root/stravia-team-isolation-verification.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)
