const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const http = require('node:http');
const {chromium} = require('playwright');

(async () => {
  const root = path.resolve(__dirname, '../ui');
  const server = http.createServer(async (req, res) => {
    const file = path.resolve(root, decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, '') || 'index.html');
    if (!file.startsWith(root + path.sep)) return res.writeHead(403).end();
    try { res.setHeader('Content-Type', {'.html':'text/html','.css':'text/css','.js':'text/javascript','.png':'image/png'}[path.extname(file)] || 'application/octet-stream'); res.end(await fs.readFile(file)); }
    catch { res.writeHead(404).end(); }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({executablePath: process.env.CGS_BROWSER || 'C:/Program Files (x86)/Microsoft/EdgeCore/157.0.4341.42/msedge.exe', headless:true});
  const errors = [];
  const base = `http://127.0.0.1:${server.address().port}/`;
  const makeReport = active => ({components:[
    {id:'vbs',name:'Virtualization-based Security (VBS)',enabled:active,state:active?'enabled':'disabled'},
    {id:'hvci',name:'Memory Integrity (HVCI)',enabled:active,state:active?'enabled':'disabled'},
    {id:'credential-guard',name:'Credential Guard',enabled:active,state:active?'enabled':'disabled'}
  ],dse:{state:'enabled'},hypervisorPresent:active,secureBoot:'true',needsUefiConsent:false,warnings:[]});
  async function open(options = {}) {
    const page = await browser.newPage({viewport:{width:1600,height:1100},reducedMotion:'reduce'});
    page.on('pageerror', error => errors.push(error.message));
    await page.route('https://**/*', route => route.abort());
    await page.addInitScript(({options,active,off}) => {
      if (options.language) localStorage.setItem('cgs-language', options.language);
      localStorage.setItem('cgs-hv-tab','hypervisor');
      const game = {id:'sample',name:'Example Game',exe_path:'C:\\Games\\Example\\Game.exe',shortcut_path:'Desktop\\Example.lnk',ready:true};
      window.fixture = {calls:[],games:[],report:options.disabled?off:active,off,result:{success:true,message:'Prepared',failed:[],restartRequired:true},elevated:options.elevated!==false,restartError:false,disableError:false,uacError:false};
      const events = {};
      window.__TAURI__ = {event:{listen:async(name,fn)=>{events[name]=fn;return()=>{}}},core:{invoke:async(name,args)=>{
        const f = window.fixture; f.calls.push({name,args});
        if (name==='platform_info') return {os:'windows',windows:true,windowsOnlyModules:[],sessionId:'test-process'};
        if (name==='check_for_update') return {available:false,currentVersion:'5.6.4',latestVersion:'5.6.4'};
        if (name==='list_games') return {games:f.games,selected_id:f.games[0]?.id || null};
        if (name==='get_app_status') return {games_count:f.games.length,selected_game:f.games[0],admin:f.elevated,game_running:false};
        if (name==='is_selected_game_running') return false;
        if (name==='select_game_exe') return options.cancelPicker?null:game.exe_path;
        if (name==='add_game') {if(options.addError)throw Error('Add failed');f.games=[game];return {games:f.games,selected_id:game.id};}
        if (name==='steam_launch_accounts') return [];
        if (name==='vbs_scan') {if(f.scanError)throw Error('WMI unavailable');return structuredClone(f.report);}
        if (name==='process_is_elevated') return f.elevated;
        if (name==='hv_disable_protections') {
          await new Promise(resolve=>setTimeout(resolve,80));
          if (f.disableError)throw Error('Disable failed');
          events['vbs-progress']?.({payload:'Protection settings saved'});
          return f.result;
        }
        if (name==='vbs_restart') {if(f.restartError)throw Error('Restart failed');return null;}
        if (name==='relaunch_as_admin') {if(f.uacError)throw Error('UAC cancelled');return null;}
        if (name==='close_window') return null;
        if (['vbs_disable','vbs_set_test_mode'].includes(name)) throw Error('Driver-boot workflow must never run');
        if (name==='local_cloud_status') return {enabled:false,managed:false,loaderSupported:true,eligibleApps:[]};
        if (name==='files_catalog') return {files:[],cached:false};
        if (name==='scan_installed_steam_games') return {games:[],libraries:[],warnings:[]};
        return {};
      }}};
    }, {options,active:makeReport(true),off:makeReport(false)});
    await page.goto(base);
    return page;
  }
  const mutations = page => page.evaluate(()=>fixture.calls.filter(c=>['hv_disable_protections','vbs_restart','vbs_disable','vbs_set_test_mode','relaunch_as_admin'].includes(c.name)));
  async function add(page) { await page.locator('.nav-item[data-view="hypervisor"]').click(); await page.locator('#hv-add-game').click(); }
  const notice = '#hv-protection-dialog';
  try {
    // Fresh English choice must persist even though English was the fallback already.
    let p = await open();
    await p.locator('#first-language-dialog').waitFor({state:'visible'});
    assert.match(await p.locator('#first-language-dialog').innerText(),/Русский/);
    if(process.env.CGS_TEST_OUTPUT)await p.locator('#first-language-dialog').screenshot({path:path.join(process.env.CGS_TEST_OUTPUT,'first-language-choice.png')});
    await p.keyboard.press('Escape'); assert(await p.locator('#first-language-dialog').isVisible());
    await p.locator('[data-first-language="en"]').click();
    assert.equal(await p.evaluate(()=>localStorage.getItem('cgs-language')),'en');
    await p.reload();assert(!await p.locator('#first-language-dialog').isVisible());await p.close();
    p = await open(); await p.locator('[data-first-language="ru"]').click();
    assert.equal(await p.evaluate(()=>window.cgsI18n.language),'ru');
    await p.reload(); assert(!await p.locator('#first-language-dialog').isVisible());
    assert.equal(await p.locator('#page-title').textContent(),'Главная');await p.close();
    p = await open({language:'ru'});assert(!await p.locator('#first-language-dialog').isVisible());await p.close();

    p = await open({language:'en'});await add(p);await p.locator(notice).waitFor({state:'visible'});
    assert.equal(await p.locator('#hv-protection-list li').count(),3);
    assert(!await p.locator('#hv-protection-consent-wrap').isVisible(),'UEFI consent is shown only when locks exist');
    assert.match(await p.locator('#hv-protection-once').innerText(),/usually done once/);
    assert.doesNotMatch(await p.locator(notice).innerText(),/F7|Test Signing|signature enforcement/);
    assert(await p.locator('[data-hv-tab-panel="hypervisor"]').evaluate(e=>e.classList.contains('active')));
    for (const theme of ['graphite','white','ocean']) {
      await p.evaluate(theme=>window.cgsPersonalization.set({theme}),theme);
      const box=await p.locator(notice).boundingBox();assert(Math.abs(box.x+box.width/2-800)<2 && Math.abs(box.y+box.height/2-550)<2);
      if(process.env.CGS_TEST_OUTPUT)await p.locator(notice).screenshot({path:path.join(process.env.CGS_TEST_OUTPUT,`hv-protection-${theme}-en.png`)});
    }
    await p.locator('#hv-protection-later').click();assert.deepEqual(await mutations(p),[]);
    assert.equal(await p.locator('#hv-games-count').textContent(),'1');await p.close();

    p = await open({language:'ru'});await add(p);await p.locator(notice).waitFor({state:'visible'});
    assert.equal(await p.locator('#hv-protection-title').innerText(),'Защиты Windows включены');
    assert.match(await p.locator('#hv-protection-accept').innerText(),/Отключить защиты и перезагрузить/);
    if(process.env.CGS_TEST_OUTPUT)await p.locator(notice).screenshot({path:path.join(process.env.CGS_TEST_OUTPUT,'hv-protection-ru.png')});
    await p.locator('#hv-protection-later').click();assert.deepEqual(await mutations(p),[]);await p.close();

    // DSE by itself is deliberately outside this new flow.
    p = await open({language:'en',disabled:true});await add(p);await p.waitForTimeout(150);
    assert(!await p.locator(notice).isVisible());assert.deepEqual(await mutations(p),[]);await p.close();
    p = await open({language:'en',cancelPicker:true});await add(p);await p.waitForTimeout(80);
    assert(!await p.locator(notice).isVisible());assert.equal(await p.evaluate(()=>fixture.calls.filter(c=>c.name==='vbs_scan').length),0);await p.close();
    p = await open({language:'en',addError:true});await add(p);await p.waitForTimeout(80);
    assert(!await p.locator(notice).isVisible());assert.equal(await p.evaluate(()=>fixture.calls.filter(c=>c.name==='vbs_scan').length),0);await p.close();

    p = await open({language:'en'});await add(p);await p.locator(notice).waitFor({state:'visible'});
    await p.evaluate(()=>{document.getElementById('hv-protection-accept').click();document.getElementById('hv-protection-accept').click();});
    await p.waitForFunction(()=>fixture.calls.some(c=>c.name==='vbs_restart'));
    let actions=await mutations(p);assert.deepEqual(actions.map(c=>c.name),['hv_disable_protections','vbs_restart']);assert.equal(actions[0].args.allowUefi,false);
    await p.locator('#hv-protection-later').click();
    await p.evaluate(()=>fixture.report=structuredClone(fixture.off));await add(p);await p.waitForTimeout(100);
    assert(!await p.locator(notice).isVisible());await p.close();

    p = await open({language:'en'});await add(p);await p.locator(notice).waitFor({state:'visible'});
    await p.evaluate(()=>fixture.report=structuredClone(fixture.off));await p.locator('#hv-protection-accept').click();
    await p.locator(notice).waitFor({state:'hidden'});assert.deepEqual(await mutations(p),[]);await p.close();

    p = await open({language:'en'});await p.evaluate(()=>fixture.result={success:false,message:'Partial result',failed:['Registry write failed']});
    await add(p);await p.locator(notice).waitFor({state:'visible'});await p.locator('#hv-protection-accept').click();
    await p.waitForFunction(()=>document.getElementById('hv-protection-status').textContent.includes('Registry write failed'));
    assert.deepEqual((await mutations(p)).map(c=>c.name),['hv_disable_protections']);await p.close();
    p = await open({language:'en'});await p.evaluate(()=>fixture.disableError=true);await add(p);await p.locator(notice).waitFor({state:'visible'});await p.locator('#hv-protection-accept').click();
    await p.waitForFunction(()=>document.getElementById('hv-protection-status').textContent.includes('Disable failed'));
    assert.deepEqual((await mutations(p)).map(c=>c.name),['hv_disable_protections']);await p.close();

    p = await open({language:'en'});await p.evaluate(()=>fixture.report.needsUefiConsent=true);await add(p);await p.locator(notice).waitFor({state:'visible'});
    assert(await p.locator('#hv-protection-accept').isDisabled());await p.locator('#hv-protection-consent').check();
    await p.locator('#hv-protection-accept').click();await p.waitForFunction(()=>fixture.calls.some(c=>c.name==='vbs_restart'));
    assert.equal((await mutations(p))[0].args.allowUefi,true);await p.close();

    p = await open({language:'en'});await p.evaluate(()=>fixture.scanError=true);await add(p);await p.locator(notice).waitFor({state:'visible'});
    assert.match(await p.locator('#hv-protection-status').innerText(),/WMI/);assert.deepEqual(await mutations(p),[]);
    await p.evaluate(()=>fixture.scanError=false);await p.locator('#hv-protection-accept').click();
    await p.waitForFunction(()=>document.getElementById('hv-protection-accept').textContent==='Disable protections & restart');assert.deepEqual(await mutations(p),[]);await p.close();

    p = await open({language:'en'});await p.evaluate(()=>fixture.restartError=true);await add(p);await p.locator(notice).waitFor({state:'visible'});await p.locator('#hv-protection-accept').click();
    await p.waitForFunction(()=>document.getElementById('hv-protection-status').textContent.includes('Restart failed'));
    await p.evaluate(()=>fixture.restartError=false);await p.locator('#hv-protection-accept').click();
    await p.waitForFunction(()=>fixture.calls.filter(c=>c.name==='vbs_restart').length===2);
    assert.equal((await mutations(p)).filter(c=>c.name==='hv_disable_protections').length,1);await p.close();

    p = await open({language:'en',elevated:false});await p.evaluate(()=>fixture.uacError=true);await add(p);await p.locator(notice).waitFor({state:'visible'});await p.locator('#hv-protection-accept').click();
    await p.waitForFunction(()=>document.getElementById('hv-protection-status').textContent.includes('UAC cancelled'));
    assert.equal(await p.evaluate(()=>localStorage.getItem('cgs-hv-protection-reopen')),null);
    assert.deepEqual((await mutations(p)).map(c=>c.name),['relaunch_as_admin']);await p.close();
    p = await open({language:'en',elevated:false});await add(p);await p.locator(notice).waitFor({state:'visible'});await p.locator('#hv-protection-accept').click();
    await p.waitForFunction(()=>fixture.calls.some(c=>c.name==='close_window'));
    assert.deepEqual((await mutations(p)).map(c=>c.name),['relaunch_as_admin']);
    await p.reload();await p.locator(notice).waitFor({state:'visible'});
    assert.deepEqual(await mutations(p),[],'UAC handoff must reopen the notice, never auto-change the system');await p.close();

    assert.deepEqual(errors,[]);console.log('PASS onboarding: first-run EN/RU persistence, add-game integration, Later, already disabled/DSE-only, failed/cancelled add, fresh scan, one action, partial/error handling, UEFI consent, restart retry, UAC cancellation and safe handoff, themes.');
  } finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
})().catch(error=>{console.error(error);process.exitCode=1;});
