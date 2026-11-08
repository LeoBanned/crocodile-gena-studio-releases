(() => {
  'use strict';
  const dialog=document.getElementById('social-hub');
  const trigger=document.getElementById('open-social-hub');
  const close=document.getElementById('social-hub-close');
  const status=document.getElementById('social-hub-status');
  const invoke=window.__TAURI__?.core?.invoke;
  const links={
    'author-telegram':'https://t.me/crocodilehub2',
    'community-telegram':'https://t.me/piratestationn',
    'author-discord':'https://discord.gg/Vwp5DAnFG',
  };
  let previousFocus;
  trigger.addEventListener('click',()=>{
    if(dialog.open)return;
    previousFocus=document.activeElement;status.textContent='';dialog.showModal();close.focus();
  });
  close.addEventListener('click',()=>dialog.close());
  dialog.addEventListener('close',()=>{if(previousFocus?.isConnected)previousFocus.focus();});
  dialog.addEventListener('click',event=>{
    if(event.target!==dialog)return;
    const box=dialog.getBoundingClientRect();
    if(event.clientX<box.left || event.clientX>box.right || event.clientY<box.top || event.clientY>box.bottom)dialog.close();
  });
  dialog.addEventListener('keydown',event=>{
    if(event.key!=='Tab')return;
    const controls=[...dialog.querySelectorAll('button:not(:disabled)')];
    const index=controls.indexOf(document.activeElement);
    event.preventDefault();
    controls[(index+(event.shiftKey?-1:1)+controls.length)%controls.length]?.focus();
  });
  dialog.querySelectorAll('[data-social]').forEach(button=>button.addEventListener('click',async()=>{
    const url=links[button.dataset.social]; if(!url)return;
    button.disabled=true;status.textContent='';
    try {if(!invoke)throw new Error('Native app required');await invoke('open_external',{url});}
    catch {status.textContent=window.cgsI18n?.t('Не удалось открыть ссылку. Попробуйте ещё раз.') || 'Could not open the link. Please try again.';}
    finally {button.disabled=false;}
  }));
})();
