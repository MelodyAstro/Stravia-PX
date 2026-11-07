"""Read public HTML and its immutable entry with a browser user agent."""
import http.client,re

def verify_hosts(hosts):
    results=[]
    def get(host,path):
        connection=http.client.HTTPSConnection(host,timeout=60)
        connection.request('GET',path,headers={'User-Agent':'Mozilla/5.0'})
        response=connection.getresponse();body=response.read();connection.close()
        assert response.status==200,f'{host} {path}: HTTP {response.status}'
        return response,body
    for host in hosts:
        response,body=get(host,'/login')
        assert 'no-store' in response.getheader('Cache-Control','')
        entry=re.search(r'(/_app/immutable/entry/[^"\s]+\.js)',body.decode())
        assert entry,'Immutable entry not found in public HTML'
        asset,data=get(host,entry.group(1))
        assert data and 'immutable' in asset.getheader('Cache-Control','')
        results.append({'host':host,'html_status':200,'asset_status':200,'html_no_store':True,'asset_immutable':True})
    return results
