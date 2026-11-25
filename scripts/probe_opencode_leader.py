#!/usr/bin/env python3
"""The OpenCode leader's bash rules on the installed OpenCode (1.19.6).

A local fixture model asks for one exact bash command per run, so only
OpenCode's permission layer decides; no account or real model is used. The
permission block mirrors opencode_worker::leader_configuration: the read
command runs, and every other command, chain, substitution, pipe and
redirect is refused without a file being written.

    python3 -I scripts/probe_opencode_leader.py [path/to/devagentics]
"""
import json, os, subprocess, sys, threading, http.server, tempfile, pathlib
root=pathlib.Path(tempfile.mkdtemp(prefix='oc-leader-probe-'))
bin=sys.argv[1] if len(sys.argv) > 1 else "devagentics"
prefix=f"{bin} leader-read probe-x"
state={"cmd":None,"n":0}
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        has_tool_result=any(m.get("role")=="tool" for m in body.get("messages",[]))
        self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.end_headers()
        def chunk(delta,finish=None):
            self.wfile.write(('data: '+json.dumps({'id':'p','object':'chat.completion.chunk','created':0,'model':'fixture','choices':[{'index':0,'delta':delta,'finish_reason':finish}]})+'\n\n').encode())
        if has_tool_result:
            chunk({'role':'assistant','content':'done'}); chunk({},'stop')
        else:
            chunk({'role':'assistant','tool_calls':[{'index':0,'id':'c1','type':'function','function':{'name':'bash','arguments':json.dumps({'command':state["cmd"],'description':'probe'})}}]})
            chunk({},'tool_calls')
        self.wfile.write(b'data: [DONE]\n\n')
srv=http.server.ThreadingHTTPServer(('127.0.0.1',0),H); threading.Thread(target=srv.serve_forever,daemon=True).start()
perm={"*":"deny","devagentics_*":"allow","bash":{"*":"deny",prefix+" *":"allow","*>*":"deny","*<*":"deny","*`*":"deny"}}
cfg={"plugin":[],"instructions":[],"autoupdate":False,"share":"disabled","command":{},"tools":{},"skills":{"paths":[],"urls":[]},"lsp":False,"formatter":False,
 "permission":perm,"agent":{"devagentics-leader":{"mode":"primary","description":"probe","options":{},"permission":perm}},"mcp":{},
 "provider":{"fixture":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":f"http://127.0.0.1:{srv.server_port}/v1","apiKey":"x"},"models":{"fixture":{"name":"fixture","tool_call":True,"limit":{"context":32000,"output":1000}}}}}}
env={k:v for k,v in os.environ.items() if not k.startswith("OPENCODE_")}
for k,sub in [('HOME','home'),('OPENCODE_TEST_HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_STATE_HOME','state'),('XDG_CACHE_HOME','cache'),('XDG_DATA_HOME','data'),('OPENCODE_CONFIG_DIR','config/opencode'),('OPENCODE_TEST_MANAGED_CONFIG_DIR','managed')]:
    (root/sub).mkdir(parents=True,exist_ok=True); env[k]=str(root/sub)
env.update(OPENCODE_DISABLE_PROJECT_CONFIG="true",OPENCODE_CONFIG_CONTENT=json.dumps(cfg),OPENCODE_DISABLE_CLAUDE_CODE="true",OPENCODE_DISABLE_DEFAULT_PLUGINS="true",OPENCODE_DISABLE_LSP_DOWNLOAD="true",OPENCODE_DISABLE_EXTERNAL_SKILLS="true")
work=root/"work"; work.mkdir()
for cmd in [prefix+" progress",prefix+" progress >> "+str(work)+"/a",prefix+" progress 2> "+str(work)+"/b",prefix+" progress &> "+str(work)+"/c",prefix+" `ls /` progress",prefix+" progress | tee "+str(work)+"/d",prefix+" progress\ntouch "+str(work)+"/e",prefix+" order 3 < /etc/passwd",prefix+" progress; ls /",prefix+" $(ls /) progress",prefix+" progress > "+str(work)+"/written.txt",prefix+" progress && touch "+str(work)+"/t",bin+" task routing probe-x off"]:
    state["cmd"]=cmd
    out=subprocess.run(["opencode","run","--format","json","--agent","devagentics-leader","--model","fixture/fixture","--","go"],cwd=work,env=env,capture_output=True,text=True,timeout=60,stdin=subprocess.DEVNULL)
    res=[]
    for line in out.stdout.splitlines():
        try: e=json.loads(line)
        except: continue
        p=e.get("part",{})
        if p.get("type")=="tool": st=p.get("state",{}); res.append((st.get("status"),(st.get("output") or st.get("error") or "")[:70].replace("\n"," ")))
    print("==",cmd.replace(bin,"BIN")); print("  ",res or out.stderr[-300:])
    allowed = cmd in (prefix+" progress",)
    status = res[0][0] if res else None
    assert status == ("completed" if allowed else "error"), (cmd, res)
assert os.listdir(work) == [], os.listdir(work)
print("OpenCode leader bash permissions hold")
