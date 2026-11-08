(() => {
  'use strict';
  const $ = id => document.getElementById(id);
  const invoke = window.__TAURI__?.core?.invoke;
  const state = {partners:[],source:'bundled',stale:false,loaded:false,busy:false,query:'',lastFetch:0};
  let renderKey = '';
  const en = () => window.cgsI18n?.language === 'en';
  const t = (ru, english) => en() ? english : ru;
  const copy = value => typeof value === 'string' ? value : (en() ? value?.en || value?.ru : value?.ru || value?.en) || '';
  const node = (tag, cls, text) => { const n=document.createElement(tag); if(cls)n.className=cls; if(text)n.textContent=text; return n; };
  const paths = {
    arrow:'M5 19 19 5M5 5h14v14',
    tools:'m14 6 4-3 3 3-3 4-4-1-9 11-3-3 11-9z',
    layers:'m12 3 9 5-9 5-9-5zM3 12l9 5 9-5M3 16l9 5 9-5',
    book:'M4 4h6l2 2 2-2h6v15h-6l-2 2-2-2H4zM12 6v15',
    people:'M16 21v-3a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v3M22 21v-3a4 4 0 0 0-3-4M16 3a4 4 0 0 1 0 8',
  };
  function icon(key) {
    const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');
    svg.setAttribute('viewBox','0 0 24 24'); svg.setAttribute('aria-hidden','true');
    const path=document.createElementNS(svg.namespaceURI,'path'); path.setAttribute('d',paths[key] || paths.layers); svg.append(path);
    if(key==='people') {const circle=document.createElementNS(svg.namespaceURI,'circle'); circle.setAttribute('cx','9');circle.setAttribute('cy','7');circle.setAttribute('r','4');svg.append(circle);}
    return svg;
  }
  function safeUrl(value, image=false) {
    try {
      const url=new URL(value);
      if(url.protocol!=='https:' || url.username || url.password || (url.port && url.port!=='443')) return '';
      const host=url.hostname.replace(/\.$/,'');
      if(!host.includes('.') || /^\[|^[\d.]+$/.test(host) || /\.(local|localhost|internal|test|invalid)$/.test(host)) return '';
      if(image && !['cdn.discordapp.com','media.discordapp.net','raw.githubusercontent.com','gist.githubusercontent.com','github.com','user-images.githubusercontent.com'].includes(host)) return '';
      return url.href;
    } catch {return '';}
  }
  function image(partner, kind, cls) {
    const src=safeUrl(kind==='logo' ? partner.logoUrl : partner.bannerUrl,true);
    const fallback=partner.id==='xero-nation' ? `./assets/partners/xero-${kind==='logo' ? 'icon' : 'banner'}.png` : '';
    if(!src && !fallback) return null;
    const img=node('img',cls); img.alt=''; img.decoding='async'; img.referrerPolicy='no-referrer';
    img.src=src || fallback;
    img.addEventListener('error',()=>{ if(fallback && !img.dataset.fallback){img.dataset.fallback='true'; img.src=fallback;}else img.remove(); });
    return img;
  }
  function tags(partner) {
    const list=node('div','partner-tags');
    (partner.tags || []).slice(0,6).forEach(tag=>list.append(node('span','',copy(tag)))); return list;
  }
  function links(partner) {
    const group=node('div','partner-links');
    (partner.links || []).slice(0,3).forEach((link,index)=>{
      const url=safeUrl(link.url); if(!url)return;
      const button=node('button',index===0?'partner-join':'partner-link'); button.type='button';
      button.append(node('span','',copy(link.label) || t('Открыть','Visit')),icon('arrow'));
      button.addEventListener('click',async()=>{
        if(!invoke)return;
        button.disabled=true;
        try {await invoke('open_external',{url});}
        catch {$('partners-status').textContent=t('Не удалось открыть ссылку. Повторите попытку.','Could not open the link. Please try again.');}
        finally {button.disabled=false;}
      });
      group.append(button);
    });
    return group;
  }
  function card(partner, featured, index) {
    const article=node('article',featured?'partner-spotlight':'partner-card'); article.dataset.partnerId=partner.id;
    const art=node('div','partner-art');
    art.append(node('span','partner-monogram',partner.name.split(/\s+/).map(s=>s[0]).slice(0,2).join('')));
    const banner=image(partner,'banner','partner-banner'); if(banner)art.append(banner);
    art.append(node('span','partner-art-index',`${String(index+1).padStart(2,'0')} / CGS PARTNER`));
    const body=node('div','partner-body');
    const identity=node('div','partner-identity'); const logoWrap=node('span','partner-logo');
    logoWrap.append(node('span','',partner.name.charAt(0)));
    const logo=image(partner,'logo',''); if(logo)logoWrap.append(logo);
    identity.append(logoWrap,node('span','partner-category',copy(partner.category) || t('Партнёр сообщества','Community partner')));
    body.append(identity,node(featured?'h2':'h3','partner-name',partner.name));
    if(copy(partner.tagline))body.append(node('p','partner-tagline',copy(partner.tagline)));
    body.append(node('p','partner-description',copy(partner.description)),tags(partner));
    const features=node('ul','partner-features');
    (partner.features || []).slice(0,featured?6:3).forEach((feature,i)=>{
      const li=node('li'); li.append(icon(['tools','layers','book','people'][i%4]),node('span','',copy(feature)));features.append(li);
    });
    if(features.children.length)body.append(features);
    body.append(links(partner));
    const url=safeUrl(partner.links?.[0]?.url);
    if(url)body.append(node('span','partner-domain',new URL(url).host + new URL(url).pathname.replace(/\/$/,'')));
    article.append(art,body); return article;
  }
  function render() {
    $('partners-heading').textContent=t('Наш круг. Ваши возможности.','Our circle. Your possibilities.');
    $('partners-intro').textContent=t('Сообщества и проекты, с которыми мы сотрудничаем. Найдите больше полезного за пределами привычного.','Communities and projects we collaborate with. Discover something useful beyond the familiar.');
    $('partners-refresh').querySelector('span').textContent=t('Обновить','Refresh');
    $('partners-refresh').disabled=state.busy;
    $('partners-search').placeholder=t('Найти партнёра…','Find a partner…');
    $('partners-search').setAttribute('aria-label',t('Поиск партнёров','Search partners'));
    $('partners-search-wrap').hidden=state.partners.length<4;
    $('partners-status').textContent=state.busy ? t('Обновляем каталог…','Updating catalog…') :
      state.stale ? t('Сохранённая копия каталога','Showing saved catalog') :
      state.source==='gist' ? t('Каталог обновлён','Catalog up to date') : t('Каталог партнёров','Partner collection');
    $('partners-status').dataset.offline=String(state.stale);
    const items=state.partners.filter(p=>`${p.name} ${copy(p.description)} ${(p.tags||[]).map(copy).join(' ')}`.toLowerCase().includes(state.query.trim().toLowerCase()));
    const area=$('partners-content'); area.setAttribute('aria-busy',String(state.busy));
    const nextKey=JSON.stringify([state.partners,state.query,en()]);
    if(nextKey===renderKey)return;
    renderKey=nextKey; area.replaceChildren();
    if(!items.length) {
      const empty=node('div','partners-empty'); empty.append(node('span','coming-label','CGS PARTNERS'),node('h3','',state.query?t('Ничего не найдено','No matches'):t('Новые знакомства — впереди','New connections ahead')),node('p','',state.query?t('Попробуйте другое название или тему.','Try another name or topic.'):t('Здесь появятся наши партнёры.','Our partners will appear here.'))); area.append(empty);
    } else {
      const featured=items.find(p=>p.featured) || (items.length===1?items[0]:null);
      if(featured)area.append(card(featured,true,state.partners.indexOf(featured)));
      const others=items.filter(p=>p!==featured);
      if(others.length){const grid=node('div','partners-grid');others.forEach(p=>grid.append(card(p,false,state.partners.indexOf(p))));area.append(grid);}
    }
    $('partners-count').textContent=`${String(items.length).padStart(2,'0')} / ${t('ПАРТНЁРЫ','PARTNERS')}`;
  }
  async function load(refresh) {
    if(!invoke || state.busy)return;
    state.busy=true; render();
    try {
      const data=await invoke('partners_catalog',{refresh});
      if(!Array.isArray(data?.partners))throw new Error('Invalid catalog');
      state.partners=data.partners; state.source=data.source;state.stale=!!data.stale;state.loaded=true;
      if(refresh)state.lastFetch=Date.now();
    } catch {state.stale=true;}
    finally {state.busy=false;render();}
  }
  async function enter() {
    if(!$('view-partners').classList.contains('active'))return;
    if(!state.loaded)await load(false);
    if(Date.now()-state.lastFetch>60000)await load(true);
  }
  $('partners-refresh').addEventListener('click',()=>load(true));
  $('partners-search').addEventListener('input',event=>{state.query=event.target.value;render();});
  window.addEventListener('cgs:language-changed',render);
  new MutationObserver(enter).observe($('view-partners'),{attributes:true,attributeFilter:['class']});
  setInterval(()=>{if(!document.hidden && $('view-partners').classList.contains('active'))load(true);},300000);
  render(); enter();
})();
