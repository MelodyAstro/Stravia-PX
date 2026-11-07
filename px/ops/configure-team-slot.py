"""Complete the authenticated Team slot using its own routes and existing Key."""
import importlib.util,json,pathlib,sqlite3
s=importlib.util.spec_from_file_location('client','/root/stravia-client.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m);c=m.Client().login()
providers=c.call('GET','/api/v1/providers')['data'];teams=[p for p in providers if p['vendor']=='openai-codex' and p['name'].startswith('GPT-Team') and p['credential_status']=='ok' and p['is_enabled']];assert len(teams)==1
provider=teams[0];assert provider['base_url']=='https://chatgpt.com/backend-api/codex'
options=dict(provider.get('vendor_options') or {})
if options.get('websocket_url')!='wss://chatgpt.com/backend-api/codex/responses':
 options['websocket_url']='wss://chatgpt.com/backend-api/codex/responses';r=c.call('PUT','/api/v1/providers/'+provider['id'],{'vendor_options':options});assert not r.get('error')
models=c.call('GET','/api/v1/models')['data'];keys=c.call('GET','/api/v1/api-keys')['data'];key=next(k for k in keys if k['name']=='GPT-Team')
backup=pathlib.Path('/root/stravia-team-slot-config-before.json');assert not backup.exists();backup.write_text(json.dumps({'key':key,'routes':models,'provider_id':provider['id']}));backup.chmod(0o600)
private_key=next(k for k in keys if k['name']=='GPT-Private');private_ids=private_key['model_ids'];all_key=next(k for k in keys if k['name']=='All');all_ids=all_key['model_ids']
view_before=c.call('GET','/api/v1/api-keys/'+private_key['id']+'/request-models')['data']['config']
inventory=c.call('GET','/api/v1/providers/'+provider['id']+'/models')['data']['models'];usable=[x for x in inventory if x['available'] and 'text' in x['route_output_modalities']];assert len(usable)==8
existing={x['model_id']:x for x in models};routes=[]
for upstream in sorted(x['id'] for x in usable):
 name='team/'+upstream
 if name in existing:
  route=existing[name];assert all(t['provider_id']==provider['id'] for t in route['targets'])
 else:
  result=c.call('POST','/api/v1/models',{'model_id':name,'display_name':'GPT-Team · '+upstream,'output_modality':'text','balance':'latency_preference','targets':[{'provider_id':provider['id'],'model':upstream,'first_token_timeout_ms':90000,'target_retry_budget':0,'target_cooldown_ms':30000}]});assert not result.get('error');route=result['data']
 routes.append(route)
ids=[r['id'] for r in routes];assert len(ids)==8 and not set(ids).intersection(private_ids)
# Add concrete permissions while disabled, then save aliases, then enable the Key.
r=c.call('PUT','/api/v1/api-keys/'+key['id'],{'model_ids':ids,'is_enabled':False});assert not r.get('error')
config={'enabled':True,'mappings':[{'request_model':r['model_id'].removeprefix('team/'),'target_model_id':r['id']} for r in routes]}
r=c.call('PUT','/api/v1/api-keys/'+key['id']+'/request-models',config);assert not r.get('error') and r['data']['config']==config
r=c.call('PUT','/api/v1/api-keys/'+key['id'],{'is_enabled':True});assert not r.get('error')
# All remains an explicit combined group. Its existing bare aliases keep Private routing.
r=c.call('PUT','/api/v1/api-keys/'+all_key['id'],{'model_ids':sorted(set(all_ids+ids))});assert not r.get('error')
current=c.call('GET','/api/v1/api-keys')['data'];assert next(k for k in current if k['name']=='GPT-Private')['model_ids']==private_ids
assert c.call('GET','/api/v1/api-keys/'+private_key['id']+'/request-models')['data']['config']==view_before
access=json.loads(m.ACCESS.read_text());updated=False
for group in access['groups']:
 if group['name']=='GPT-Team':group['enabled']=True;group['models']=[r['model_id'] for r in routes];updated=True
assert updated;m.ACCESS.write_text(json.dumps(access,indent=2));m.ACCESS.chmod(0o600)
report={'configured':True,'provider':provider['name'],'provider_id':provider['id'],'websocket_url':options['websocket_url'],'team_models':[r['model_id'] for r in routes],'team_short_names':[x['request_model'] for x in config['mappings']],'key_preserved':True,'private_binding_unchanged':True,'all_has_explicit_team_access':True}
pathlib.Path('/root/stravia-team-slot-configuration.json').write_text(json.dumps(report,indent=2));print(json.dumps(report),flush=True)