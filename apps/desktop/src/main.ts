import './styles.css';
import { bindCardDragging } from './card-drag';
import { compareCardOrder, planCardMove } from './card-order';
import { createLiveEditor } from './live-editor';
import type { EditorView } from '@codemirror/view';
let liveEditor:EditorView|null=null;
import { renderObsidianMarkdown, renderObsidianMarkdownBlocks, hydrateMarkdown, findNote } from './markdown';
import { motionValue, springValue, styleEffect, type MotionValue } from 'motion';

export type Column = string;
export interface ColumnInfo { id: string; name: string }
export interface Card { id:string; title:string; body:string; column:Column; labels:string[]; labelColors?:Record<string,string>; updatedAt:number; revision?:string; position?:number; due?:string; start?:string }
export interface Conflict { cardId:string; local:string; remote:string; base:string; parentRevisionIds?:string[]; localCard?:Card; remoteCard?:Card; localTombstone?:boolean; remoteTombstone?:boolean }
export interface BackendCard { id:string; title:string; body:string; column:string; labels?:string[]; label_colors?:Record<string,string>; updated_at?:string; revision?:string; position?:number; due?:string; start?:string }
export interface BackendEvent { path:string; kind:string; valid:boolean; duplicate:boolean; error?:string }
export interface ArchivedCard { card:BackendCard; revision_id:string; archived_at:number }
export interface BoardInfo { path:string; board_id:string; title?:string; columns?:ColumnInfo[]; labels?:Record<string,string>; cards:string[] }
export interface OpenBoard { path:string; boardId:string; title:string }
export type ModelProvider = "huggingface" | "ollama" | "openai";
export interface LocalModel { id:string; provider:ModelProvider; name:string; source:string; path?:string; size_bytes:number }
export interface HuggingFaceModel { id:string; downloads:number; likes:number; last_modified?:string }
export interface ModelSettings { provider?:ModelProvider; model_id?:string; ollama_url?:string; openai_base_url?:string; openai_api_key?:string; keep_model_loaded?:boolean }
export interface AppCapabilities { local_ai:boolean; remote_ai:boolean; mobile:boolean }
export const capabilities:AppCapabilities={local_ai:false,remote_ai:false,mobile:false};
let capabilitiesLoaded=false;
const hasLocalAI=()=>capabilitiesLoaded&&capabilities.local_ai;
const hasRemoteAI=()=>capabilitiesLoaded&&capabilities.remote_ai;
const hasAI=()=>hasLocalAI()||hasRemoteAI();
function providerAvailable(provider:ModelProvider|undefined){return provider==='huggingface'?hasLocalAI():provider==='ollama'||provider==='openai'?hasRemoteAI():false;}
function availableProvider():ModelProvider|undefined{return hasLocalAI()?'huggingface':hasRemoteAI()?'ollama':undefined;}
export interface DiagnosticState { pending:boolean; error:string; result:string; address:string }
export const diagnostics:DiagnosticState={pending:false,error:'',result:'',address:''};
let diagnosticDialQueued=false;
export async function loadCapabilities(){try{const value=await invoke<Partial<AppCapabilities>>('app_capabilities');capabilities.local_ai=value.local_ai===true;capabilities.remote_ai=value.remote_ai===true;capabilities.mobile=value.mobile===true;capabilitiesLoaded=true;state.error='';}catch{capabilities.local_ai=false;capabilities.remote_ai=false;capabilities.mobile=false;capabilitiesLoaded=false;state.error='';}render();}
export interface OpenAIModel { id:string; created?:number; owned_by?:string }
export interface RecommendedModel { repo:string; file:string; size:string; note:string }
export interface DownloadProgress { filename:string; downloaded_bytes:number; total_bytes?:number|null; bytes_per_second:number }
export const recommendedModels:RecommendedModel[]=[
  {repo:"unsloth/Qwen3.5-4B-GGUF",file:"Qwen3.5-4B-Q4_K_M.gguf",size:"~2.6 GB",note:"Best tested accuracy"},
  {repo:"numind/NuExtract3-GGUF",file:"NuExtract3-Q4_K_M.gguf",size:"~2.8 GB",note:"Fast extraction specialist"},
  {repo:"unsloth/Qwen3-4B-Instruct-2507-GGUF",file:"Qwen3-4B-Instruct-2507-Q4_K_M.gguf",size:"~2.5 GB",note:"Strong general Quick Add model"},
  {repo:"bartowski/Qwen_Qwen3-1.7B-GGUF",file:"Qwen_Qwen3-1.7B-Q4_K_M.gguf",size:"~1.2 GB",note:"Fastest recommended option"},
  {repo:"Qwen/Qwen2.5-1.5B-Instruct-GGUF",file:"qwen2.5-1.5b-instruct-q4_k_m.gguf",size:"~1.1 GB",note:"Legacy lightweight option"},
  {repo:"bartowski/Qwen2.5-3B-Instruct-GGUF",file:"Qwen2.5-3B-Instruct-Q4_K_M.gguf",size:"~2.0 GB",note:"Older JSON-capable model"},
  {repo:"bartowski/Llama-3.2-3B-Instruct-GGUF",file:"Llama-3.2-3B-Instruct-Q4_K_M.gguf",size:"~2.0 GB",note:"Strong general extraction"},
  {repo:"bartowski/SmolLM2-1.7B-Instruct-GGUF",file:"SmolLM2-1.7B-Instruct-Q4_K_M.gguf",size:"~1.1 GB",note:"Fastest option"},
  {repo:"bartowski/Phi-3.5-mini-instruct-GGUF",file:"Phi-3.5-mini-instruct-Q4_K_M.gguf",size:"~2.2 GB",note:"Reliable structured output"},
  {repo:"bartowski/Qwen2.5-7B-Instruct-GGUF",file:"Qwen2.5-7B-Instruct-Q4_K_M.gguf",size:"~4.7 GB",note:"Best quality under 5 GB"},
  {repo:"bartowski/Mistral-7B-Instruct-v0.3-GGUF",file:"Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",size:"~4.4 GB",note:"Mature and dependable"},
  {repo:"bartowski/gemma-2-9b-it-GGUF",file:"gemma-2-9b-it-Q4_K_M.gguf",size:"~5.8 GB",note:"Higher-quality parsing"},
  {repo:"bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF",file:"DeepSeek-R1-Distill-Qwen-7B-Q4_K_M.gguf",size:"~4.7 GB",note:"Strong reasoning, slower"},
  {repo:"bartowski/Qwen2.5-14B-Instruct-GGUF",file:"Qwen2.5-14B-Instruct-Q4_K_M.gguf",size:"~9.0 GB",note:"Maximum quality for 18 GB RAM"},
];

type View = 'board' | 'settings';
type DialogState =
  | {kind:'input'; action:'rename-board'|'rename-column'|'add-column'|'pair'; title:string; label:string; value:string; targetId?:string; multiline?:boolean}
  | {kind:'confirm-delete'; cardId:string; title:string};
const defaultColumns = ():ColumnInfo[] => [
  {id:'backlog',name:'Backlog'},
  {id:'doing',name:'In progress'},
  {id:'done',name:'Done'},
];

export type DueDateDisplay = 'calendar' | 'countdown';
export const DUE_DATE_DISPLAY_STORAGE_KEY = 'knot.due-date-display';

interface CalendarDateParts { year:number; month:number; day:number }
function parseCalendarDate(value:string):CalendarDateParts|null {
  const match=/^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if(!match)return null;
  const parts={year:Number(match[1]),month:Number(match[2]),day:Number(match[3])};
  const date=new Date(0);
  date.setHours(12,0,0,0);
  date.setFullYear(parts.year,parts.month-1,parts.day);
  return date.getFullYear()===parts.year&&date.getMonth()===parts.month-1&&date.getDate()===parts.day?parts:null;
}

export function loadDueDateDisplayPreference():DueDateDisplay {
  try{
    const value=window.localStorage.getItem(DUE_DATE_DISPLAY_STORAGE_KEY);
    return value==='countdown'?'countdown':'calendar';
  }
  catch{return 'calendar';}
}
export function saveDueDateDisplayPreference(display:DueDateDisplay):void {
  try{window.localStorage.setItem(DUE_DATE_DISPLAY_STORAGE_KEY,display);}catch{/* Storage may be unavailable. */}
}
export function formatDueDate(value:string,display:DueDateDisplay='calendar',now=new Date()):string {
  const date=parseCalendarDate(value);
  if(!date)return value;
  if(display!=='countdown'){
    const calendarDate=new Date(0);
    calendarDate.setHours(12,0,0,0);
    calendarDate.setFullYear(date.year,date.month-1,date.day);
    return new Intl.DateTimeFormat('en',{month:'short',day:'numeric'}).format(calendarDate);
  }
  const today=new Date(0);
  today.setUTCFullYear(now.getFullYear(),now.getMonth(),now.getDate());
  today.setUTCHours(0,0,0,0);
  const due=new Date(0);
  due.setUTCFullYear(date.year,date.month-1,date.day);
  due.setUTCHours(0,0,0,0);
  const difference=Math.round((due.getTime()-today.getTime())/86400000);
  if(difference===0)return 'Today';
  if(difference>0)return `in ${difference} day${difference===1?'':'s'}`;
  const overdue=-difference;
  return `${overdue} day${overdue===1?'':'s'} overdue`;
}
export function setDueDateDisplayPreference(display:DueDateDisplay):void {
  state.dueDateDisplay=display==='countdown'?'countdown':'calendar';
  saveDueDateDisplayPreference(state.dueDateDisplay);
  render();
}

export const state = {
  boardPath:null as string|null,
  boardId:'',
  boardTitle:'Untitled board',
  columns:defaultColumns(),
  openBoards:[] as OpenBoard[],
  view:'board' as View,
  dueDateDisplay:loadDueDateDisplayPreference(),
  cards:[] as Card[],
  selected:null as string|null,
  conflicts:[] as Conflict[],
  connected:false,
  peer:'No devices paired',
  peerId:'',
  peerAddress:'',
  endpointId:'',
  endpointAddress:'',
  connection:'offline',
  error:'',
  loading:false,
  dialog:null as DialogState|null,
  modelSettings:{} as ModelSettings,
  localModels:[] as LocalModel[],
  ollamaModels:[] as {name:string;size_bytes:number;modified_at?:string}[],
  openaiModels:[] as OpenAIModel[],
  modelLoading:false,
  modelFeedback:'',
  downloadProgress:null as DownloadProgress|null,
  hfSearchResults:[] as HuggingFaceModel[],
  quickAddOpen:false,
  labelColors:{} as Record<string,string>,
  labels:{} as Record<string,string>,
  showArchived:false,
  archivedLoading:false,
  archivedCards:[] as {card:Card;revisionId:string;archivedAt:number}[],
  drafts:{} as Record<string,Record<string,Partial<Card>>>, // boardPath -> cardId -> draft
};
function labelColor(label:string, colors?:Record<string,string>):string {
  const value=colors?.[label]||state.labelColors[label]||'#8b5cf6';
  if(/[;{}]|url\s*\(|expression\s*\(/i.test(value)) return '#8b5cf6';
  const syntax=/^(?:#[0-9a-f]{3,8}|(?:rgb|hsl)a?\([0-9.,%\s]+\)|(?:color|lab|lch|oklab|oklch|hwb)\([0-9a-z.,%\s/-]+\)|[a-z]{1,24})$/i;
  const supported=typeof CSS!=='undefined'&&typeof CSS.supports==='function'?CSS.supports('color',value):false;
  return syntax.test(value)&&(supported||/^#[0-9a-f]{3,8}$/i.test(value)||/^[a-z]{1,24}$/i.test(value))?value:'#8b5cf6';
}

const app = document.querySelector<HTMLDivElement>('#app') ?? (()=>{const el=document.createElement('div');el.id='app';document.body.append(el);return el;})();
const esc=(s:string)=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!));
const message=(e:unknown)=>e instanceof Error?e.message:String(e);
type InvokeFn = <T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
let invokeImpl:InvokeFn|null=null;
export function setInvokeForTests(fn:InvokeFn|null){invokeImpl=fn;}
async function invoke<T>(command:string,args?:Record<string,unknown>):Promise<T>{if(invokeImpl)return invokeImpl<T>(command,args);const api=await import('@tauri-apps/api/core');return api.invoke<T>(command,args);}
let toastMessage='';
let lastToastSource='';
let toastTimer:number|null=null;
function syncToast(){
  if(state.error){
    if(state.error!==lastToastSource){
      lastToastSource=state.error;
      toastMessage=state.error;
      if(toastTimer!==null)window.clearTimeout(toastTimer);
      toastTimer=window.setTimeout(()=>{toastMessage='';toastTimer=null;render();},5000);
    }
  }else{
    lastToastSource='';
  }
}
function toast(){return toastMessage?`<div class="toast" role="alert" aria-live="assertive"><span>${esc(toastMessage)}</span><button type="button" class="toast-dismiss" aria-label="Dismiss notification">×</button></div>`:'';}
type DirectoryPicker=()=>Promise<string|null>;
let directoryPickerForTests:DirectoryPicker|null=null;
export function setDirectoryPickerForTests(fn:DirectoryPicker|null){directoryPickerForTests=fn;}

export async function chooseBoardDirectory():Promise<string|null>{
  if(directoryPickerForTests)return directoryPickerForTests();
  try{const {open}=await import('@tauri-apps/plugin-dialog');const selected=await open({directory:true,multiple:false,title:'Open or create a Knot board'});return typeof selected==='string'?selected:null;}
  catch(e){const error=new Error('Board directory picker unavailable: '+message(e));error.name='DirectoryPickerUnavailable';throw error;}
}

export function normalize(x:BackendCard):Card{return {id:x.id,title:x.title,body:x.body,column:x.column||'backlog',labels:x.labels||[],updatedAt:x.updated_at?Date.parse(x.updated_at):Date.now(),revision:x.revision,position:x.position,due:x.due,start:x.start,labelColors:x.label_colors||{}};}
export function renderMarkdown(value:string, currentId?:string):string {
  return renderObsidianMarkdown(value,{notes:state.cards.map(note=>note.id===currentId?{...note,body:value}:note),currentId});
}
const attachmentUrls=new Map<string,Promise<string>>();
let attachmentBoard:string|null=null;
async function hydrateCardMarkdown(root:ParentNode){
  void hydrateMarkdown(root);
  if(attachmentBoard!==state.boardPath){attachmentUrls.forEach(url=>void url.then(URL.revokeObjectURL,()=>{}));attachmentUrls.clear();attachmentBoard=state.boardPath;}
  const path=state.boardPath;if(!path)return;
  const mime:Record<string,string>={png:'image/png',jpg:'image/jpeg',jpeg:'image/jpeg',gif:'image/gif',webp:'image/webp',avif:'image/avif',bmp:'image/bmp',svg:'image/svg+xml',mp3:'audio/mpeg',wav:'audio/wav',ogg:'audio/ogg',m4a:'audio/mp4',mp4:'video/mp4',webm:'video/webm',mov:'video/quicktime',pdf:'application/pdf'};
  await Promise.all(Array.from(root.querySelectorAll<HTMLElement>('[data-attachment],[data-attachment-link]')).map(async el=>{
    const relative=el.dataset.attachment||el.dataset.attachmentLink!;
    const type=mime[relative.split('.').pop()!.toLowerCase()];if(!type)return;
    let url=attachmentUrls.get(relative);
    if(!url){url=invoke<ArrayBuffer|number[]>('read_attachment',{path,relative}).then(bytes=>URL.createObjectURL(new Blob([bytes instanceof ArrayBuffer?bytes:new Uint8Array(bytes)],{type})));attachmentUrls.set(relative,url);}
    try{const source=await url;if(!el.isConnected)return;if(el instanceof HTMLAnchorElement){el.href=source;el.target='_blank';el.rel='noopener noreferrer';if(el.dataset.pdf){const frame=document.createElement('iframe');frame.className='pdf-embed';frame.src=source;frame.title=relative;el.after(frame);}}else el.setAttribute('src',source);}
    catch{attachmentUrls.delete(relative);el.setAttribute('title','Attachment unavailable: '+relative);el.classList.add('is-unresolved');}
  }));
}
export function sharePath(path:string, root?:string):string {
  const normalized=path.replaceAll('\\','/');
  if(normalized.split('/').includes('..')) throw new Error('Path traversal is not allowed');
  if(root){ const base=root.replaceAll('\\','/').replace(/\/$/,''); const relative=normalized.startsWith(base+'/')?normalized.slice(base.length):normalized; return relative.startsWith('/')?relative:`/${relative}`; }
  return normalized.startsWith('/')?normalized:`/${normalized}`;
}
export function sanitizeFilenamePart(value:string):string {
  const encoded=new TextEncoder().encode(value.replace(/[^A-Za-z0-9_]+/g,'-').replace(/^-+|-+$/g,''));
  return new TextDecoder().decode(encoded.slice(0,60)).replace(/-+$/g,'');
}
export function shareCardPath(_boardPath:string, cardId:string, card?:Pick<Card,'column'|'title'>):string {
  const safeId=cardId;
  if(!/^[A-Za-z0-9_-]{1,200}$/.test(safeId)) throw new Error('Invalid card ID');
  const current=card||(_boardPath===state.boardPath?state.cards.find(item=>item.id===cardId):undefined);
  const root=_boardPath.replaceAll('\\','/').replace(/\/+$/,'');
  if(!current) return `${root}/cards/${safeId}.md`;
  return `${root}/cards/${sanitizeFilenamePart(current.column)||'card'}-${sanitizeFilenamePart(current.title)||'card'}-${safeId}.md`;
}
// Editing changes updatedAt, not a card's place on the board. IDs break ties stably.
function orderedCards(column:Column){return state.cards.filter(card=>card.column===column).sort(compareCardOrder);}
function updateTab(){if(!state.boardPath)return;const tab={path:state.boardPath,boardId:state.boardId,title:state.boardTitle};const index=state.openBoards.findIndex(x=>x.path===state.boardPath);if(index<0)state.openBoards.push(tab);else state.openBoards[index]=tab;}
export function applyBoardInfo(info:BoardInfo){
  if(!info.board_id)throw new Error('Backend did not return a stable board ID');
  const previousBoardId=state.boardId;const previousBoardPath=state.boardPath;state.boardPath=info.path;state.boardId=info.board_id;state.boardTitle=info.title?.trim()||info.path.split(/[\/]/).filter(Boolean).pop()||'My board';state.columns=info.columns?.length?info.columns:defaultColumns();state.labels=info.labels||{};state.labelColors={...state.labels};if(previousBoardPath!==info.path){state.archivedCards=[];state.archivedLoading=false;}if(previousBoardId&&previousBoardId!==info.board_id){state.peerId='';state.peerAddress='';state.selected=null;}updateTab();
}
export function setView(view:View){state.view=view;}

export async function loadCards(){const path=state.boardPath;if(!path)return;state.loading=true;render();try{const result=await invoke<BackendCard[]>('list_cards',{path});if(state.boardPath!==path)return;state.cards=mergeDraftsIntoCards((result||[]).map(normalize));state.error='';}catch(e){if(state.boardPath===path)state.error='Unable to load cards: '+message(e);}finally{if(state.boardPath===path){state.loading=false;render();}}}
export async function loadArchivedCards(){const path=state.boardPath;if(!path)return;state.archivedLoading=true;render();try{const result=await invoke<ArchivedCard[]>('list_archived_cards',{path});if(state.boardPath!==path)return;state.archivedCards=(result||[]).map(item=>({card:normalize(item.card),revisionId:item.revision_id,archivedAt:item.archived_at}));state.error='';}catch(e){if(state.boardPath===path)state.error='Unable to load archived cards: '+message(e);}finally{if(state.boardPath===path){state.archivedLoading=false;render();}}}
async function restoreArchivedCard(item:{card:Card;revisionId:string;archivedAt:number}){if(!state.boardPath)return;try{const restored=await invoke<BackendCard>('restore_card',{path:state.boardPath,id:item.card.id,revisionId:item.revisionId});state.cards=[...state.cards,normalize(restored)];state.archivedCards=state.archivedCards.filter(x=>x.revisionId!==item.revisionId);state.error='Card restored';render();}catch(e){state.error='Unable to restore card: '+message(e);render();}}
export async function parseQuickAdd(text:string):Promise<ParsedCardDraft>{if(!hasAI())throw new Error('AI capability unavailable');if(!state.boardPath)throw new Error('Open a board before using Quick Add');return invoke<ParsedCardDraft>('parse_quick_add',{path:state.boardPath,text});}

export interface ParsedCardDraft {title:string;body:string;column:string;labels:string[];label_colors?:Record<string,string>;due?:string;start?:string;confidence:number;warnings:string[]}

export async function loadModels(){
  if(!capabilitiesLoaded)await loadCapabilities();
  if(!capabilitiesLoaded||!hasAI())return;
  try{
    state.modelSettings=await invoke<ModelSettings>('model_settings');
    const persisted=state.modelSettings.provider;
    if(persisted&&!providerAvailable(persisted)){
      const next=availableProvider();
      state.modelSettings={...state.modelSettings,provider:next,model_id:undefined};
      state.modelFeedback=`${persisted==='huggingface'?'On this Mac':persisted==='ollama'?'Ollama':'OpenAI API'} is unavailable. Choose ${next==='huggingface'?'On this Mac':next==='ollama'?'Ollama':'OpenAI API'} instead.`;
    } else if(!persisted){
      const next=availableProvider();
      if(next)state.modelSettings={...state.modelSettings,provider:next};
    }
    state.localModels=hasLocalAI()?await invoke<LocalModel[]>('list_local_models'):[];
    if(hasRemoteAI()&&state.modelSettings.provider==='ollama')state.ollamaModels=await invoke<{name:string;size_bytes:number;modified_at?:string}[]>('list_ollama_models',{url:state.modelSettings.ollama_url});
    else if(hasRemoteAI()&&state.modelSettings.provider==='openai'){if(state.modelSettings.openai_api_key)await loadOpenAIModels();}
    else if(hasLocalAI()&&state.modelSettings.provider==='huggingface'&&state.modelSettings.keep_model_loaded&&state.modelSettings.model_id)await invoke('load_local_model',{id:state.modelSettings.model_id});
    render();
  }catch(e){state.error='Unable to read model settings: '+message(e);render();}
}
export async function searchHuggingFace(query:string){if(!hasLocalAI()||!query.trim())return;try{state.modelLoading=true;render();state.hfSearchResults=await invoke<HuggingFaceModel[]>("search_huggingface_models",{query:query.trim(),limit:20});state.error="";}catch(e){state.error="Unable to search Hugging Face: "+message(e);}finally{state.modelLoading=false;render();}}
export async function loadOllamaModels(url=state.modelSettings.ollama_url||'http://127.0.0.1:11434'){if(!hasRemoteAI())return;try{state.modelSettings={...state.modelSettings,ollama_url:url};state.modelLoading=true;render();state.ollamaModels=await invoke<{name:string;size_bytes:number;modified_at?:string}[]>('list_ollama_models',{url});state.error='';}catch(e){state.error='Unable to connect to Ollama: '+message(e);}finally{state.modelLoading=false;render();}}
export async function loadOpenAIModels(baseUrl=state.modelSettings.openai_base_url||'https://openrouter.ai/api/v1',apiKey=state.modelSettings.openai_api_key){if(!hasRemoteAI())return;try{state.modelSettings={...state.modelSettings,openai_base_url:baseUrl,openai_api_key:apiKey};state.modelLoading=true;render();state.openaiModels=await invoke<OpenAIModel[]>('list_openai_models',{baseUrl,apiKey});state.error='';}catch(e){state.error='Unable to connect to the OpenAI-compatible API: '+message(e);}finally{state.modelLoading=false;render();}}
export async function checkOpenAIAccess(baseUrl=state.modelSettings.openai_base_url||'https://openrouter.ai/api/v1',apiKey=state.modelSettings.openai_api_key){if(!hasRemoteAI())return;try{state.modelSettings={...state.modelSettings,openai_base_url:baseUrl,openai_api_key:apiKey};state.modelLoading=true;state.modelFeedback='';render();await invoke('check_openai_access',{baseUrl,apiKey});await invoke('save_model_settings',{settings:state.modelSettings});state.error='';state.modelFeedback='Access confirmed. Your API key is valid and saved.';}catch(e){state.modelFeedback='';state.error='Access check failed: '+message(e);}finally{state.modelLoading=false;render();}}
export function applyDownloadProgress(progress:DownloadProgress){state.downloadProgress=progress;if(state.modelLoading)render();}
export async function downloadHFModel(repo_id:string,filename:string){if(!hasLocalAI())return;try{state.modelLoading=true;state.downloadProgress={filename,downloaded_bytes:0,total_bytes:null,bytes_per_second:0};state.modelFeedback=`Installing ${filename}…`;state.error='';render();const model=await invoke<LocalModel>('download_huggingface_gguf',{repoId:repo_id,filename});state.localModels=[...state.localModels.filter(item=>item.id!==model.id),model];await saveModelSettings({provider:'huggingface',model_id:model.id});state.modelFeedback=`${model.name} is installed and ready.`;}catch(e){state.error='Unable to install model: '+message(e);state.modelFeedback='';}finally{state.modelLoading=false;state.downloadProgress=null;render();}}

export async function saveModelSettings(settings:ModelSettings){if(!hasAI()||(settings.provider&&!providerAvailable(settings.provider)))return;try{const next={...state.modelSettings,...settings};await invoke('save_model_settings',{settings:next});state.modelSettings=next;await loadModels();render();}catch(e){state.error='Unable to save model settings: '+message(e);render();}}
export async function setKeepModelLoaded(keep:boolean){if(!hasLocalAI()||state.modelSettings.provider!=='huggingface')return;try{state.modelLoading=true;state.modelFeedback=keep?'Loading model…':'Unloading model…';render();const settings={...state.modelSettings,keep_model_loaded:keep};await invoke('save_model_settings',{settings});if(keep&&settings.provider==='huggingface'&&settings.model_id)await invoke('load_local_model',{id:settings.model_id});else if(!keep)await invoke('unload_local_model');state.modelSettings=settings;state.modelFeedback=keep?'Model loaded and ready.':'Model unloaded. It will load only when Quick Add needs it.';}catch(e){state.error='Unable to change model loading: '+message(e);}finally{state.modelLoading=false;render();}}
export async function deleteAllModels(){if(!hasLocalAI()||!window.confirm('Delete all locally downloaded models?'))return;try{await invoke('unload_local_model');await invoke('delete_all_local_models');state.localModels=[];if(state.modelSettings.provider==='huggingface'){state.modelSettings={...state.modelSettings,model_id:undefined};await invoke('save_model_settings',{settings:state.modelSettings});}state.modelFeedback='Downloaded models removed.';render();}catch(e){state.error='Unable to delete local models: '+message(e);render();}}

export async function loadSync(){try{const [s,endpoint,peers]=await Promise.all([invoke<{connected:boolean;trusted_peers:number;endpoint_id?:string;address?:string;connection:string}>('sync_status'),invoke<{endpoint_id:string;address:string}>('endpoint_info'),invoke<{peer_id:string;trusted:boolean;address?:string}[]>('list_trusted_peers')]);state.connected=!!s.connected;state.connection=s.connection;state.endpointId=endpoint.endpoint_id||s.endpoint_id||'';state.endpointAddress=endpoint.address||s.address||'';const trusted=peers.filter(x=>x.trusted);state.peer=trusted.map(x=>x.peer_id).join(', ')||'No devices paired';if(!state.peerId&&trusted.length)state.peerId=trusted[0].peer_id;if(!state.peerAddress&&trusted.length)state.peerAddress=trusted.find(x=>x.peer_id===state.peerId)?.address||'';}catch(e){state.error='Unable to read sync status: '+message(e);}render();}
function endpointIdFromAddress(address:string):string{const parsed=JSON.parse(address) as {id?:string};if(!parsed.id||typeof parsed.id!=='string')throw new Error('Endpoint address does not contain an id');return parsed.id;}
export async function pairAddress(address:string){if(!state.boardPath||!state.boardId)throw new Error('Open a board with a stable board ID before pairing');const peerId=endpointIdFromAddress(address);const peer=await invoke<{peer_id:string;trusted:boolean}>('pair_peer_address',{peerId,address,authorizedBoards:[state.boardId]});if(!peer.trusted)throw new Error('Peer was not trusted');state.peerId=peer.peer_id;state.peerAddress=address;await loadSync();return peer;}
export async function syncNow(){if(!state.boardPath||!state.boardId){state.error='Open a board with a stable board ID before syncing.';render();return;}if(!state.peerId||!state.peerAddress){state.error='Pair a peer address before syncing.';render();return;}state.loading=true;render();try{const result=await invoke<{status:string;transferred:number;received:number;conflicts?:{card_id:string;local_revision_id:string;remote_revision_id:string;parent_revision_ids:string[];local?:BackendCard;remote?:BackendCard;local_tombstone:boolean;remote_tombstone:boolean}[]}>('sync_board',{path:state.boardPath,peerId:state.peerId,address:state.peerAddress});const successfulRoute=['connected','direct','relay'].includes(result.status);await loadCards();state.conflicts=(result.conflicts||[]).map(conflict=>({cardId:conflict.card_id,local:'Local revision '+conflict.local_revision_id,remote:'Remote revision '+conflict.remote_revision_id,base:'Sync conflict',parentRevisionIds:conflict.parent_revision_ids,localCard:conflict.local?normalize(conflict.local):state.cards.find(card=>card.id===conflict.card_id),remoteCard:conflict.remote?normalize(conflict.remote):undefined,localTombstone:conflict.local_tombstone,remoteTombstone:conflict.remote_tombstone}));await loadSync();state.error=successfulRoute?'':result.status==='conflict'?'Sync completed with unresolved conflicts.':`Sync finished with status: ${result.status}`;if(successfulRoute){state.connected=true;state.connection=result.status;}render();}catch(e){state.error='Sync failed: '+message(e);state.loading=false;render();}}

export async function openDefaultBoard(){state.loading=true;state.error='';render();try{const info=await invoke<BoardInfo>('open_default_board');applyBoardInfo(info);state.view='board';try{await invoke('watch_board',{path:state.boardPath});}catch{/* Watch failure must not discard loaded mobile board. */}await loadCards();await loadSync();}catch(e){state.loading=false;state.error='Unable to load default board: '+message(e);render();}}
export async function openBoard(){let p:string|null;try{p=await chooseBoardDirectory();}catch(e){state.error=message(e);render();if(e instanceof Error&&e.name==='DirectoryPickerUnavailable'&&capabilities.mobile)await openDefaultBoard();return;}if(!p)return;state.loading=true;state.error='';render();try{const info=await invoke<BoardInfo>('open_or_create_board',{path:p});applyBoardInfo(info);state.view='board';try{await invoke('watch_board',{path:state.boardPath});}catch{/* Watcher is optional; board remains usable without it. */}await loadCards();await loadSync();}catch(e){state.error='Unable to open board: '+message(e);state.loading=false;render();}}
export async function switchBoard(path:string){if(path===state.boardPath){state.view='board';render();return;}state.loading=true;state.error='';render();try{const info=await invoke<BoardInfo>('open_board',{path});applyBoardInfo(info);state.view='board';try{await invoke('watch_board',{path});}catch{/* Watcher is optional; board remains usable. */}await loadCards();}catch(e){state.error='Unable to switch board: '+message(e);state.loading=false;render();}}
function applyMetadata(info:BoardInfo){applyBoardInfo(info);state.error='';render();}
export async function renameBoard(title:string){if(!state.boardPath||!title.trim())return;try{applyMetadata(await invoke<BoardInfo>('rename_board',{path:state.boardPath,title:title.trim()}));}catch(e){state.error='Unable to rename board: '+message(e);render();}}
export async function renameColumn(columnId:string,name:string){if(!state.boardPath||!name.trim())return;try{applyMetadata(await invoke<BoardInfo>('rename_column',{path:state.boardPath,columnId,name:name.trim()}));}catch(e){state.error='Unable to rename column: '+message(e);render();}}
export async function createColumn(name:string){if(!state.boardPath||!name.trim())return;try{applyMetadata(await invoke<BoardInfo>('create_column',{path:state.boardPath,name:name.trim()}));}catch(e){state.error='Unable to create column: '+message(e);render();}}
export async function createLabel(name:string,color:string){if(!state.boardPath||!name.trim())return;try{applyMetadata(await invoke<BoardInfo>('create_label',{path:state.boardPath,name:name.trim(),color}));}catch(e){state.error='Unable to create label: '+message(e);render();}}
export async function updateLabel(name:string,newName:string,color:string){if(!state.boardPath)return;try{applyMetadata(await invoke<BoardInfo>('update_label',{path:state.boardPath,name,newName,color}));}catch(e){state.error='Unable to update label: '+message(e);render();}}
export async function deleteLabel(name:string){if(!state.boardPath)return;try{applyMetadata(await invoke<BoardInfo>('delete_label',{path:state.boardPath,name}));}catch(e){state.error='Unable to delete label: '+message(e);render();}}
export async function reorderColumns(columnIds:string[]){if(!state.boardPath||columnIds.length!==state.columns.length||new Set(columnIds).size!==state.columns.length||columnIds.some(id=>!state.columns.some(column=>column.id===id)))return;const previous=[...state.columns];const byId=new Map(state.columns.map(column=>[column.id,column]));state.columns=columnIds.map(id=>byId.get(id)!);render();try{applyMetadata(await invoke<BoardInfo>('reorder_columns',{path:state.boardPath,columnIds}));}catch(e){state.columns=previous;state.error='Unable to reorder columns: '+message(e);render();}}

async function add(column:Column){if(!state.boardPath){state.error='Open or create a board before adding cards.';render();return;}try{const saved=await invoke<BackendCard>('add_card',{path:state.boardPath,input:{title:'New card',body:'Add a description…',column,labels:[],position:1000}});state.cards.push(normalize(saved));state.selected=saved.id;state.error='';render();}catch(e){state.error='Unable to add card: '+message(e);render();}}
const pendingSaves = new Set<string>();
const pendingResaves = new Set<string>();
async function save(c:Card) {
  const boardPath=state.boardPath;
  if(!boardPath)return;
  const key=JSON.stringify([boardPath,c.id]);
  if(pendingSaves.has(key)){pendingResaves.add(key);return;}
  const existing=state.drafts[boardPath]?.[c.id]||{};
  const draft:Partial<Card>={
    title:document.querySelector<HTMLInputElement>('#title')?.value??existing.title??c.title,
    body:document.querySelector<HTMLTextAreaElement>('#body')?.value??existing.body??c.body,
    column:document.querySelector<HTMLSelectElement>('#column')?.value??existing.column??c.column,
    labels:Array.from(document.querySelector<HTMLSelectElement>('#labels')?.selectedOptions||[]).map(option=>option.value),
    labelColors:{...(c.labelColors||{}),...(existing.labelColors||{}),...state.labelColors},
    due:document.querySelector<HTMLInputElement>('#due')?.value??existing.due??'',
    start:document.querySelector<HTMLInputElement>('#start')?.value??existing.start??'',
  };
  (state.drafts[boardPath]??={})[c.id]=draft;
  const input={title:draft.title,body:draft.body,column:draft.column,labels:draft.labels,label_colors:draft.labelColors,due:draft.due,start:draft.start,position:c.position};
  pendingSaves.add(key);
  try {
    const saved=await invoke<BackendCard>('update_card',{path:boardPath,id:c.id,input});
    // Every edit replaces the draft object. Never clear edits made during this request.
    const unchanged=state.drafts[boardPath]?.[c.id]===draft;
    if(unchanged)delete state.drafts[boardPath][c.id];
    if(state.boardPath===boardPath){
      state.cards=state.cards.map(x=>x.id===c.id?normalize(saved):x);
      if(unchanged&&state.selected===c.id)state.selected=null;
      state.error='';
      render();
    }
  } catch(e) {
    if(state.boardPath===boardPath){state.error='Unable to save card: '+message(e);render();}
  } finally {
    pendingSaves.delete(key);
    if(pendingResaves.delete(key)){
      const latest=state.cards.find(card=>card.id===c.id);
      if(latest)queueMicrotask(()=>void save(latest));
    }
  }
}
async function remove(id:string){if(!state.boardPath)return;try{await invoke<boolean>('delete_card',{path:state.boardPath,id});state.cards=state.cards.filter(x=>x.id!==id);state.selected=null;state.error='';render();}catch(e){state.error='Unable to delete card: '+message(e);render();}}
function showDialog(dialog:DialogState){state.dialog=dialog;render();queueMicrotask(()=>document.querySelector<HTMLElement>('#dialog-input, #dialog-confirm')?.focus());}
function closeDialog(){state.dialog=null;render();}
function requestDelete(cardId:string){showDialog({kind:'confirm-delete',cardId,title:'Delete card?'});}
function beginInlineEdit(trigger:HTMLElement,value:string,inputId:string,label:string,persist:(next:string)=>Promise<void>){const input=document.createElement('input');input.id=inputId;input.className='inline-title-input';input.value=value;input.setAttribute('aria-label',label);trigger.replaceWith(input);input.focus();input.select();let finished=false;const finish=(save:boolean)=>{if(finished)return;finished=true;const next=input.value.trim();if(save&&next&&next!==value)void persist(next);else render();};input.addEventListener('blur',()=>finish(true));input.addEventListener('keydown',e=>{if(e.key==='Enter'){e.preventDefault();finish(true);}else if(e.key==='Escape'){e.preventDefault();finish(false);}});}
async function submitInputDialog(value:string){const dialog=state.dialog;if(!dialog||dialog.kind!=='input'||!value.trim())return;state.dialog=null;render();switch(dialog.action){case'rename-board':await renameBoard(value);break;case'rename-column':if(dialog.targetId)await renameColumn(dialog.targetId,value);break;case'add-column':await createColumn(value);break;case'pair':try{await pairAddress(value.trim());state.error='';render();}catch(e){state.error='Pairing failed: '+message(e);render();}break;}}
let cardMovePending=false;
export async function moveCardToSlot(id:string,column:Column,beforeId:string|null){
  const path=state.boardPath;
  if(!path||cardMovePending||!state.columns.some(item=>item.id===column))return;
  const moves=planCardMove(state.cards,id,column,beforeId);
  if(!moves.length)return;
  let confirmed=state.cards;
  const planned=new Map(moves.map(move=>[move.id,move]));
  state.cards=state.cards.map(card=>({...card,...planned.get(card.id)}));
  cardMovePending=true;render();
  let succeeded=false;
  try{
    // Most drops write one card. Exhausted/tied ranks need a bounded rebalance.
    // Keep each confirmed write so partial failures never pretend to roll back disk.
    for(const move of moves){
      const saved=normalize(await invoke<BackendCard>('move_card',{path,...move}));
      confirmed=confirmed.map(card=>card.id===saved.id?saved:card);
    }
    if(state.boardPath===path){state.cards=mergeDraftsIntoCards(confirmed);state.error='';succeeded=true;}
  }catch(error){
    if(state.boardPath===path){
      state.cards=mergeDraftsIntoCards(confirmed);
      try{
        const cards=await invoke<BackendCard[]>('list_cards',{path});
        if(state.boardPath===path)state.cards=mergeDraftsIntoCards(cards.map(normalize));
      }catch{/* Retain confirmed writes if reloading is unavailable. */}
      if(state.boardPath===path)state.error='Unable to complete card move: '+message(error);
    }
  }finally{
    cardMovePending=false;
    if(state.boardPath===path){
      render();
      const card=Array.from(app.querySelectorAll<HTMLElement>('.card')).find(card=>card.dataset.id===id);
      card?.focus({preventScroll:true});
      if(succeeded){
        const cards=orderedCards(column),index=cards.findIndex(card=>card.id===id);
        const status=document.createElement('div');status.className='board-move-status';status.setAttribute('role','status');status.setAttribute('aria-live','polite');app.append(status);
        status.textContent=`${cards[index]?.title||'Card'} moved to position ${index+1} of ${cards.length} in ${state.columns.find(item=>item.id===column)?.name||column}`;
      }
    }
  }
}
export async function moveCard(id:string,column:Column,position=1000){if(!state.boardPath)return;const old=state.cards.find(x=>x.id===id);if(!old)return;const previous=old.column,previousPosition=old.position;state.cards=state.cards.map(x=>x.id===id?{...x,column,position}:x);render();try{const saved=await invoke<BackendCard>('move_card',{path:state.boardPath,id,column,position});state.cards=state.cards.map(x=>x.id===id?normalize(saved):x);state.error='';render();}catch(e){state.cards=state.cards.map(x=>x.id===id?{...x,column:previous,position:previousPosition}:x);state.error='Unable to move card: '+message(e);render();}}
async function resolveConflict(choice:string){const x=state.conflicts[0];if(!x||!state.boardPath)return;const local=x.localCard||state.cards.find(v=>v.id===x.cardId);const remote=x.remoteCard||local;const deleteChoice=(choice==='local'&&x.localTombstone)||(choice==='remote'&&x.remoteTombstone);if((!local&&!deleteChoice)||(!remote&&!deleteChoice)||!x.parentRevisionIds||x.parentRevisionIds.length<2){state.error='Conflict resolution needs two parent revision IDs.';render();return;}const payload=(card:Card)=>({title:card.title,body:card.body,column:card.column,labels:card.labels,position:card.position??1000,due:card.due,start:card.start});const fallback:Card={id:x.cardId,title:'Deleted card',body:'',column:'backlog',labels:[],updatedAt:0};const localPayload=payload(local||remote||fallback);const remotePayload=payload(remote||local||fallback);try{const saved=await invoke<BackendCard>('resolve_conflict',{path:state.boardPath,id:x.cardId,resolution:{choice,local:localPayload,remote:remotePayload,manual:choice==='manual'?localPayload:null,tombstone:!!deleteChoice,parentRevisionIds:x.parentRevisionIds}});state.cards=deleteChoice?state.cards.filter(v=>v.id!==x.cardId):state.cards.map(v=>v.id===x.cardId?normalize(saved):v);state.conflicts=state.conflicts.filter(conflict=>conflict.cardId!==x.cardId);state.error='';render();}catch(e){state.error='Conflict resolution failed: '+message(e);render();}}
export async function poll(){if(!state.boardPath||cardMovePending)return;try{const events=await invoke<BackendEvent[]>('poll_watch_events');if(events.length){const bad=events.find(x=>!x.valid&&x.error);if(bad)state.error=bad.error!;const conflict=events.find(x=>x.error?.toLowerCase().includes('conflict'));if(conflict){const stem=conflict.path.split('/').pop()?.replace(/\.md$/,'')||'';const match=/(?:^|-)([0-7][0-9A-HJKMNP-TV-Z]{25})$/i.exec(stem);const id=match?.[1]||stem;const local=state.cards.find(c=>c.id===id);state.conflicts=[{cardId:id,local:local?JSON.stringify(local,null,2):'Local version unavailable',remote:conflict.error||'Remote version payload unavailable',base:'Base version unavailable',localCard:local,parentRevisionIds:local?.revision?[local.revision]:[]}];}await loadCards();}}catch(e){state.error='Unable to poll board changes: '+message(e);render();}}
export function mergeDraftsIntoCards(cards:Card[]):Card[]{const boardDrafts=state.drafts[state.boardPath||''];if(!boardDrafts||Object.keys(boardDrafts).length===0)return cards;return cards.map(card=>{const draft=boardDrafts[card.id];if(!draft)return card;return {...card,title:draft.title??card.title,body:draft.body??card.body,column:draft.column??card.column,labels:draft.labels??card.labels,due:draft.due!==undefined?draft.due:card.due,start:draft.start!==undefined?draft.start:card.start,labelColors:draft.labelColors??card.labelColors};});}

function quickAddShortcut(e:KeyboardEvent){const target=e.target as HTMLElement|null;if(hasAI()&&(e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='n'&&!target?.closest('input,textarea,select,[contenteditable=\"true\"]')){e.preventDefault();openQuickAdd();}}
document.addEventListener('keydown',quickAddShortcut);

function quickAddModal(){return `<div class="modal-backdrop"><form class="app-dialog quick-add" id="quick-add-form" role="dialog" aria-modal="true"><h2>Quick Add</h2><input id="quick-add-input" autocomplete="off" placeholder="Task due:tomorrow in:doing #label" aria-label="Quick add task"/><p class="notice">Enter creates the card · Shift+Enter creates and opens it · Escape cancels</p><div class="editor-actions"><button type="button" class="secondary" id="quick-add-cancel">Cancel</button><button type="submit" class="primary">Create</button></div></form></div>`;}
async function openQuickAdd(){if(!state.boardPath){state.error='Open a board before using Quick Add.';render();return;}if(!capabilitiesLoaded)await loadCapabilities();if(!capabilitiesLoaded||!hasAI())return;const provider=state.modelSettings.provider;if(provider&&!providerAvailable(provider)){state.view='settings';state.error='That AI provider is unavailable. Choose an available provider in Settings.';render();return;}if(!provider||!state.modelSettings.model_id){state.view='settings';state.error='Choose an AI provider and model in Settings before using Quick Add.';render();return;}state.quickAddOpen=true;render();queueMicrotask(()=>document.querySelector<HTMLInputElement>('#quick-add-input')?.focus());}
let quickAddSubmitting=false;
async function submitQuickAdd(text:string,openEditor:boolean){
  if(quickAddSubmitting||!text.trim())return;
  quickAddSubmitting=true;
  const form=document.querySelector<HTMLFormElement>('#quick-add-form');
  form?.setAttribute('aria-busy','true');
  form?.querySelectorAll<HTMLInputElement|HTMLButtonElement>('input,button').forEach(control=>{control.disabled=true;});
  const submit=form?.querySelector<HTMLButtonElement>('button[type=submit]');
  if(submit)submit.textContent='Creating…';
  try{const draft=await parseQuickAdd(text);const saved=await invoke<BackendCard>('add_card',{path:state.boardPath,input:{title:draft.title,body:draft.body,column:draft.column,labels:draft.labels,...(draft.label_colors&&Object.keys(draft.label_colors).length?{label_colors:draft.label_colors}:{}),due:draft.due,start:draft.start,position:1000}});state.cards.push(normalize(saved));state.quickAddOpen=false;state.selected=openEditor?saved.id:null;state.error=draft.warnings.length?draft.warnings.join(' '):'';render();}catch(e){state.error='Quick Add failed: '+message(e);render();}finally{quickAddSubmitting=false;}
}

const editorIcon=(name:'edit'|'close'|'title'|'description'|'column'|'labels'|'calendar'|'flag'|'eye'|'trash'|'archive'|'share'|'check'|'more'|'plus')=>{
  const paths:Record<string,string>={
    edit:'<path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L8 18l-4 1 1-4Z"/>',
    close:'<path d="m18 6-12 12M6 6l12 12"/>',
    title:'<path d="M4 7V4h16v3M9 20h6M12 4v16"/>',
    description:'<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><path d="M14 2v6h6M8 13h8M8 17h6"/>',
    column:'<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18M15 3v18"/>',
    labels:'<path d="M20.6 13.1 11 22.7a2.4 2.4 0 0 1-3.4 0l-6.3-6.3a2.4 2.4 0 0 1 0-3.4L10.9 3.4A2 2 0 0 1 12.3 3H19a2 2 0 0 1 2 2v6.7a2 2 0 0 1-.4 1.4Z"/><circle cx="16" cy="8" r="1"/>',
    calendar:'<rect width="18" height="18" x="3" y="4" rx="2"/><path d="M16 2v4M8 2v4M3 10h18"/>',
    flag:'<path d="M5 22V4M5 4h11l-1 5 1 5H5"/>',
    eye:'<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>',
    trash:'<path d="M3 6h18M8 6V4h8v2M19 6l-1 15H6L5 6M10 11v6M14 11v6"/>',
    archive:'<path d="M4 7h16v13H4zM3 4h18v3H3zM9 11h6"/>',
    share:'<circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><path d="m8.6 10.5 6.8-4M8.6 13.5l6.8 4"/>',
    check:'<path d="m20 6-11 11-5-5"/>',
    more:'<circle cx="5" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="19" cy="12" r="1.4" fill="currentColor" stroke="none"/>',
    plus:'<path d="M12 5v14M5 12h14"/>'
  };
  return `<svg class="editor-icon" viewBox="0 0 24 24" aria-hidden="true">${paths[name]}</svg>`;
};
function prettyEditorDate(value:string|undefined):string{
  if(!value)return '';
  return formatDueDate(value,'calendar');
}
function editor(c:Card){
  const labels=Array.from(new Set([...Object.keys(state.labels),...c.labels]));
  const startText=prettyEditorDate(c.start),dueText=prettyEditorDate(c.due);
  const dateText=startText&&dueText?`${startText} → ${dueText}`:(startText||dueText||'Dates');
  const chips=c.labels.map(label=>`<button type="button" class="ce-label-chip" data-pop="labelPop" aria-expanded="false"><span class="ce-label-dot" style="background:${esc(labelColor(label,c.labelColors))}"></span><span>${esc(label)}</span></button>`).join('');
  return `<div class="modal-backdrop editor-backdrop"><main class="editor card-editor" role="dialog" aria-modal="true" aria-label="Edit card">
    <header class="ce-header">
      <input id="title" class="ce-title" value="${esc(c.title)}" placeholder="Card title" aria-label="Card title"/>
      <div class="ce-header-actions">
        <button type="button" class="ce-icon-btn" id="share" title="Share" aria-label="Share">${editorIcon('share')}</button>
        <button type="button" class="ce-icon-btn" id="archive" title="Archive card" aria-label="Archive card">${editorIcon('archive')}</button>
        <button type="button" class="ce-icon-btn" id="close" title="Close" aria-label="Close">${editorIcon('close')}</button>
      </div>
    </header>
    <nav class="ce-strip" aria-label="Card properties">
      <span class="ce-label-cluster">${chips}</span>
      <button type="button" class="ce-property" data-pop="datePop" aria-expanded="false">${editorIcon('calendar')}<span id="ce-date-text">${esc(dateText)}</span></button>
      <button type="button" class="ce-property" data-pop="addPop" aria-expanded="false" aria-label="Add property">${editorIcon('plus')}<span>Add</span></button>
      <span class="ce-strip-spacer"></span>
    </nav>
    <section class="ce-md-shell"><div class="ce-md-inner"><textarea id="body" hidden aria-hidden="true" tabindex="-1">${esc(c.body)}</textarea><div id="card-live-editor"></div></div></section>
    <select id="column" hidden aria-hidden="true" tabindex="-1">${state.columns.map(column=>`<option value="${esc(column.id)}" ${c.column===column.id?'selected':''}>${esc(column.name)}</option>`).join('')}</select>
    <select id="labels" multiple hidden aria-hidden="true" tabindex="-1">${labels.map(label=>`<option value="${esc(label)}" ${c.labels.includes(label)?'selected':''}>${esc(label)}</option>`).join('')}</select>
    <button type="button" id="save" hidden aria-hidden="true">Save</button>
    <button type="button" id="delete" hidden aria-hidden="true">Delete</button>
    <div class="ce-popover" id="labelPop" role="menu"><div class="ce-pop-label">Labels</div>${labels.map(label=>`<div class="ce-menu-row"><button type="button" class="ce-menu-item ${c.labels.includes(label)?'selected':''}" data-toggle-label="${esc(label)}"><span class="ce-swatch" style="background:${esc(labelColor(label,c.labelColors))}"></span>${esc(label)}<span class="ce-check">${editorIcon('check')}</span></button><input type="color" data-card-label-color="${esc(label)}" value="${esc(labelColor(label,c.labelColors))}" aria-label="${esc(label)} color" title="Change ${esc(label)} color"/></div>`).join('')||'<div class="ce-pop-label">No labels yet — add them in Settings</div>'}</div>
    <div class="ce-popover" id="listPop" role="menu"><div class="ce-pop-label">Move to</div>${state.columns.map(column=>`<button type="button" class="ce-menu-item ${c.column===column.id?'selected':''}" data-set-column="${esc(column.id)}">${editorIcon('column')}${esc(column.name)}<span class="ce-check">${editorIcon('check')}</span></button>`).join('')}</div>
    <div class="ce-popover ce-date-pop" id="datePop" role="dialog" aria-label="Edit dates"><div class="ce-pop-label">Dates</div><div class="ce-date-row"><label for="start">Start</label><input id="start" type="date" value="${esc(c.start||'')}"/></div><div class="ce-date-row"><label for="due">Due</label><input id="due" type="date" value="${esc(c.due||'')}"/></div></div>
    <div class="ce-popover" id="addPop" role="menu"><div class="ce-pop-label">Properties</div><button type="button" class="ce-menu-item" data-open-pop="labelPop">${editorIcon('labels')}Labels</button><button type="button" class="ce-menu-item" data-open-pop="listPop">${editorIcon('column')}Move to list</button></div>

  </main></div>`;
}
function closeEditorPopovers(except?:HTMLElement){document.querySelectorAll<HTMLElement>('.ce-popover.open').forEach(pop=>{if(pop!==except)pop.classList.remove('open');});document.querySelectorAll<HTMLElement>('.card-editor [data-pop]').forEach(btn=>{if(!except||btn.dataset.pop!==except.id)btn.setAttribute('aria-expanded','false');});}
function placeEditorPopover(trigger:HTMLElement,pop:HTMLElement){
  if(pop.parentElement!==document.body)document.body.appendChild(pop);
  const r=trigger.getBoundingClientRect(),gap=6;
  pop.style.display='block';
  const pr=pop.getBoundingClientRect();
  let left=r.left,top=r.bottom+gap;
  if(left+pr.width>window.innerWidth-10)left=window.innerWidth-pr.width-10;
  if(top+pr.height>window.innerHeight-10)top=r.top-pr.height-gap;
  pop.style.left=Math.max(10,left)+'px';
  pop.style.top=Math.max(10,top)+'px';
  pop.style.display='';
}
function conflictModal(x:Conflict){const ready=(x.parentRevisionIds?.length||0)>=2;return `<div class="modal-backdrop"><section class="conflict" role="dialog" aria-modal="true"><div class="editor-head"><span>CONFLICT DETECTED</span></div><h2>Conflict requires review</h2><p>${esc(x.remote)}</p>${!ready?'<p class="error">Resolution awaits two parent revision IDs from the backend.</p>':''}<div class="editor-actions"><button data-resolve="local" ${ready?'':'disabled'}>Keep local</button><button data-resolve="remote" ${ready?'':'disabled'}>Keep remote</button><button data-resolve="manual" ${ready?'':'disabled'}>Manual merge</button></div></section></div>`;}
function settingsToggle(){const settings=state.view==='settings';return `<button class="settings-toggle reveal-action utility-action settings-action ${settings?'active':''}" data-view="${settings?'board':'settings'}" aria-label="${settings?'Return to board':'Open settings'}" title="${settings?'Return to board':'Settings'}"><span class="action-label">${settings?'Board':'Settings'}</span><span class="plus-icon">${settings?'←':'⚙'}</span></button>`;}
function utilityActions(){return `<div class="utility-actions">${state.boardPath&&state.view==='board'?`<button type="button" id="show-archived" class="create-action reveal-action utility-action archive-action"><span class="action-label">${state.showArchived?'Hide archived':'Show archived'}</span><span class="plus-icon">${editorIcon('archive')}</span></button>`:''}${settingsToggle()}</div>`;}
function archivedCardsIn(column:Column){return state.archivedCards.filter(item=>item.card.column===column).sort((a,b)=>compareCardOrder(a.card,b.card));}
function visibleCardsIn(column:Column){
  const active=orderedCards(column).map(card=>({card,archived:null as {card:Card;revisionId:string;archivedAt:number}|null}));
  const archived=state.showArchived?archivedCardsIn(column).map(item=>({card:item.card,archived:item})):[];
  return [...active,...archived].sort((a,b)=>compareCardOrder(a.card,b.card));
}
function boardCard(item:{card:Card;archived:{card:Card;revisionId:string;archivedAt:number}|null}){
  const {card,archived}=item;
  return `<article class="card ${archived?'archived-card':''} ${state.selected===card.id?'selected':''}" tabindex="0" aria-label="${esc(archived?'Archived card: '+card.title+'. Restore to edit.':card.title+'. Hold Alt and use arrow keys to move.') }" data-id="${esc(card.id)}" ${archived?'data-archived="true"':''}>${archived?`<div class="card-topline"><span class="archived-badge">Archived</span></div>`:''}<h3>${esc(card.title)}</h3><div class="card-body markdown-rendered">${renderMarkdown(card.body,card.id)}</div><div class="card-foot">${card.due?`<time class="due-date" datetime="${esc(card.due)}" aria-label="Due ${esc(formatDueDate(card.due,state.dueDateDisplay))}"><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M16 3v4M8 3v4M3 10h18"/></svg>${esc(formatDueDate(card.due,state.dueDateDisplay))}</time>`:''}${card.labels.map(l=>`<span class="tag" style="--label-color:${esc(labelColor(l,card.labelColors))}">${esc(l)}</span>`).join('')}</div>${archived?`<button type="button" class="secondary archived-restore" data-restore-archived="${esc(archived.revisionId)}">Restore</button>`:''}</article>`;
}
function boardPage(){if(!state.boardPath)return '<section class="workspace empty"><div><h1>Open a board to get started</h1><p>Choose a folder to open or create a board.</p><button class="primary" id="empty-open">Open board</button></div></section>';return `<section class="workspace" data-page="board"><div class="toolbar"><div class="board-title-menu"><div class="title-row"><h1><button class="editable-heading" id="rename-board" aria-label="Rename board" title="Click to rename">${esc(state.boardTitle)}</button></h1></div><div class="board-switcher" role="menu" aria-label="Open boards">${state.openBoards.filter(board=>board.path!==state.boardPath).map(board=>`<button role="menuitem" data-board-path="${esc(board.path)}" class="board-menu-item ${board.path===state.boardPath?'active':''}">${esc(board.title)}</button>`).join('')}<button role="menuitem" class="board-menu-item board-menu-open" id="open" aria-label="Open or create board">＋</button></div></div><div class="toolbar-actions"><span class="status board-sync-status ${state.connected?'online':'offline'}" role="status"><i></i>${state.connected?'Synced':'Offline'}</span><div class="create-actions" role="group" aria-label="Create">${hasAI()?'<button class="create-action card-action reveal-action" id="new-card" aria-label="Add card" title="Add card"><span class="action-label">Add card</span><span class="plus-icon">＋</span></button>':''}<button class="create-action column-action reveal-action" id="add-column" aria-label="Add column" title="Add column"><span class="action-label">Add column</span><span class="plus-icon">＋</span></button></div></div></div>${state.loading?'<p class="notice">Loading…</p>':''}<div class="board-scroll"><div class="columns">${state.columns.map(column=>`<section class="column" data-column="${esc(column.id)}"><div class="column-head"><div><div class="column-title"><h2><button class="editable-column-heading" data-rename-column="${esc(column.id)}" aria-label="Rename ${esc(column.name)}" title="Click to rename">${esc(column.name)}</button> <b>${visibleCardsIn(column.id).length}</b></h2></div></div><button class="add reveal-action" data-add="${esc(column.id)}" aria-label="Add card to ${esc(column.name)}"><span class="action-label">Add card</span><span class="plus-icon">＋</span></button></div><div class="dropzone" data-drop-column="${esc(column.id)}" role="group" aria-label="${esc(column.name)} cards">${visibleCardsIn(column.id).map(boardCard).join('')}</div></section>`).join('')}</div></div></section>`;}
function appDialog(dialog:DialogState){if(dialog.kind==='confirm-delete')return `<div class="modal-backdrop"><section class="app-dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title"><h2 id="dialog-title">Delete card?</h2><p>This action cannot be undone.</p><div class="editor-actions"><button class="secondary" id="dialog-cancel">Cancel</button><button class="primary danger-action" id="dialog-confirm">Delete card</button></div></section></div>`;const field=dialog.multiline?`<textarea id="dialog-input" aria-label="${esc(dialog.label)}">${esc(dialog.value)}</textarea>`:`<input id="dialog-input" aria-label="${esc(dialog.label)}" value="${esc(dialog.value)}"/>`;return `<div class="modal-backdrop"><form class="app-dialog" id="input-dialog-form" role="dialog" aria-modal="true" aria-labelledby="dialog-title"><h2 id="dialog-title">${esc(dialog.title)}</h2><label>${esc(dialog.label)}${field}</label><div class="editor-actions"><button type="button" class="secondary" id="dialog-cancel">Cancel</button><button type="submit" class="primary">${dialog.action==='pair'?'Pair device':'Save'}</button></div></form></div>`;}

function formatBytes(bytes:number){if(!bytes)return 'Size unknown';const units=['B','KB','MB','GB'];const index=Math.min(Math.floor(Math.log(bytes)/Math.log(1024)),units.length-1);return `${(bytes/1024**index).toFixed(index>1?1:0)} ${units[index]}`;}
function modelName(id:string){return state.localModels.find(model=>model.id===id)?.name||state.ollamaModels.find(model=>model.name===id)?.name||state.openaiModels.find(model=>model.id===id)?.id||id;}
function modelSettingsSection(){
  const usingOpenAI=hasRemoteAI()&&state.modelSettings.provider==='openai';
  const usingOllama=hasRemoteAI()&&(state.modelSettings.provider==='ollama'||(!hasLocalAI()&&!usingOpenAI));
  const active=state.modelSettings.model_id;
  const installed=state.localModels.map(model=>`<button type="button" class="model-row ${active===model.id?'selected':''}" data-select-model="${esc(model.id)}"><span class="model-icon">${active===model.id?'✓':'◇'}</span><span><strong>${esc(model.name)}</strong><small>${formatBytes(model.size_bytes)} · On this Mac</small></span><b>${active===model.id?'Active':'Use'}</b></button>`).join('');
  const recommended=recommendedModels.slice(0,4).map((model,index)=>`<article class="model-card ${index===0?'featured':''}"><div><h3>${esc(model.note)}</h3><p>${esc(model.repo.split('/').pop()?.replace(/-GGUF$/i,'')||model.repo)} · ${esc(model.size)}</p></div><button type="button" class="${index===0?'primary':'secondary'}" aria-label="Install ${esc(model.repo.split('/').pop()||model.repo)}" data-install-recommended data-repo="${esc(model.repo)}" data-file="${esc(model.file)}" ${state.modelLoading?'disabled':''}>${state.modelLoading?'Please wait…':'Install'}</button></article>`).join('');
  const ollama=state.ollamaModels.length?state.ollamaModels.map(model=>`<button type="button" class="model-row ${active===model.name?'selected':''}" data-select-ollama="${esc(model.name)}"><span class="model-icon">${active===model.name?'✓':'◇'}</span><span><strong>${esc(model.name)}</strong><small>${formatBytes(model.size_bytes)} · Ollama</small></span><b>${active===model.name?'Active':'Use'}</b></button>`).join(''):'<div class="empty-models"><strong>No Ollama models found</strong><span>Start Ollama or install a model, then refresh above.</span></div>';
  const openai=state.modelSettings.openai_api_key?state.openaiModels.length?state.openaiModels.map(model=>`<button type="button" class="model-row ${active===model.id?'selected':''}" data-select-openai="${esc(model.id)}"><span class="model-icon">${active===model.id?'✓':'◇'}</span><span><strong>${esc(model.id)}</strong><small>OpenAI-compatible API${model.owned_by?' · '+esc(model.owned_by):''}</small></span><b>${active===model.id?'Active':'Use'}</b></button>`).join(''):'<div class="empty-models"><strong>No models found</strong><span>Check the endpoint, API key, or enter a model ID below.</span></div>':'<div class="empty-models"><strong>API key required</strong><span>Add your API key and connect to list models.</span></div>';
  const openaiPanel=`<div class="model-panel"><div class="panel-title"><h3>OpenAI-compatible API</h3><button type="button" class="text-button" id="refresh-openai">Connect</button><button type="button" class="text-button" id="check-openai-access">Check access</button></div>${state.modelFeedback?`<p class="model-feedback" role="status">${esc(state.modelFeedback)}</p>`:''}<div class="api-connection"><label>Base URL<input id="openai-base-url" value="${esc(state.modelSettings.openai_base_url||'https://openrouter.ai/api/v1')}"/></label><label>API key<input id="openai-api-key" type="password" value="${esc(state.modelSettings.openai_api_key||'')}"/></label></div>${state.openaiModels.length&&state.modelSettings.openai_api_key?'<input type="search" class="model-filter" aria-label="Filter API models" placeholder="Find a model…" data-model-filter="api-model-list"/>':''}<div class="installed-models model-list" id="api-model-list">${openai}</div><p class="model-filter-empty" hidden>No matching models.</p><div class="custom-install manual-api-model"><label>Model ID<input id="openai-model" placeholder="e.g. anthropic/claude-sonnet-4"/></label><button type="button" class="secondary" id="use-openai-model">Use model</button></div></div>`;
  return `<section class="settings-section model-settings" data-settings-section="models" id="settings-models" tabindex="-1"><div class="section-heading"><div><span class="eyebrow">QUICK ADD</span><h2>AI model</h2><p>Choose what powers natural-language card creation.</p></div><div class="model-setup-status ${active?'ready':''}"><i></i><span><strong>${active?'Ready':'Choose a model'}</strong><small>${active?esc(modelName(active)):'Install a recommended option below'}</small></span></div></div><div class="provider-switch" role="tablist" aria-label="Model source">${hasLocalAI()?`<button type="button" role="tab" data-model-provider="huggingface" aria-selected="${!usingOllama&&!usingOpenAI}">On this Mac</button>`:''}${hasRemoteAI()?`<button type="button" role="tab" data-model-provider="ollama" aria-selected="${usingOllama}">Ollama</button><button type="button" role="tab" data-model-provider="openai" aria-selected="${usingOpenAI}">OpenAI API</button>`:''}</div>${usingOpenAI||!capabilities.local_ai||state.modelSettings.provider!=='huggingface'?'':`<label class="model-retention"><input type="checkbox" id="keep-model-loaded" ${state.modelSettings.keep_model_loaded?'checked':''} ${!active||state.modelLoading?'disabled':''}/><span><strong>Keep model loaded</strong><small>${state.modelSettings.keep_model_loaded?'Loaded for faster Quick Add':'Unload after each Quick Add to save memory'}</small></span></label>`}${usingOllama?`<div class="model-panel"><div class="panel-title"><h3>Available in Ollama</h3><button type="button" class="text-button" id="refresh-ollama">Refresh</button></div>${state.ollamaModels.length?'<input type="search" class="model-filter" aria-label="Filter Ollama models" placeholder="Find a model…" data-model-filter="ollama-model-list"/>':''}<div class="installed-models model-list" id="ollama-model-list">${ollama}</div><p class="model-filter-empty" hidden>No matching models.</p><details class="advanced-settings"><summary>Connection</summary><label>Ollama URL<input id="ollama-url" value="${esc(state.modelSettings.ollama_url||'http://127.0.0.1:11434')}"/></label></details></div>`:usingOpenAI?openaiPanel:`<div class="model-panel">${state.modelFeedback?`<p class="model-feedback" role="status">${esc(state.modelFeedback)}</p>`:''}${state.modelLoading&&state.downloadProgress?`<div class="download-progress" role="status" aria-live="polite"><div><strong>Downloading ${esc(state.downloadProgress.filename)}</strong><span>${formatBytes(state.downloadProgress.downloaded_bytes)}${state.downloadProgress.total_bytes?' of '+formatBytes(state.downloadProgress.total_bytes):''}</span></div><progress max="100" ${state.downloadProgress.total_bytes?`value="${Math.min(100,state.downloadProgress.downloaded_bytes/state.downloadProgress.total_bytes*100)}"`:''}>${state.downloadProgress.total_bytes?Math.round(state.downloadProgress.downloaded_bytes/state.downloadProgress.total_bytes*100):0}%</progress><div class="download-meta"><span>${state.downloadProgress.total_bytes?Math.round(state.downloadProgress.downloaded_bytes/state.downloadProgress.total_bytes*100)+'%':'Preparing…'}</span><b>${state.downloadProgress.bytes_per_second?formatBytes(state.downloadProgress.bytes_per_second)+'/s':'Connecting…'}</b></div></div>`:''}${installed?`<div class="panel-title"><h3>Installed</h3><span>${state.localModels.length} ${state.localModels.length===1?'model':'models'}</span></div><div class="installed-models model-list">${installed}</div>`:''}<div class="panel-title discover-title"><div><h3>Recommended</h3><span>One click installs and activates</span></div></div><div class="model-gallery">${recommended}</div><details class="advanced-settings" ${state.hfSearchResults.length?'open':''}><summary>Find another Hugging Face model</summary><form class="hf-search" id="hf-search-form"><input id="hf-search" placeholder="Search GGUF models" aria-label="Search Hugging Face models"/><button type="submit" class="secondary">Search</button></form>${state.hfSearchResults.length?`<div class="search-results">${state.hfSearchResults.map(model=>`<div class="search-result"><a href="https://huggingface.co/${encodeURI(model.id)}" target="_blank" rel="noreferrer"><strong>${esc(model.id)}</strong><small>${model.downloads.toLocaleString()} downloads</small></a><button type="button" class="text-button" data-use-hf="${esc(model.id)}">Choose</button></div>`).join('')}</div>`:''}<div class="custom-install"><label>Repository<input id="hf-repo" placeholder="owner/model"/></label><label>Exact .gguf file<input id="hf-file" placeholder="model.Q4_K_M.gguf"/></label><button type="button" class="secondary" id="download-hf-model">Install and use</button></div></details>${state.localModels.length?`<details class="danger-zone"><summary>Manage storage</summary><button class="danger" id="delete-all-models">Delete all downloaded models</button></details>`:''}</div>`}</section>`;
}
function dueDateSettingsSection(){
  const calendarSelected=state.dueDateDisplay==='calendar';
  return `<section class="settings-section due-date-settings" data-settings-section="card-dates" id="settings-card-dates" tabindex="-1"><div class="section-heading"><div><span class="eyebrow">BOARD</span><h2>Card dates</h2><p>Choose how due dates appear on cards. Calendar date is the default.</p></div></div><div class="due-date-options" role="radiogroup" aria-label="Due date display"><label class="due-date-option"><input type="radio" name="due-date-display" value="calendar" data-due-date-display="calendar" ${calendarSelected?'checked':''}/><span><strong>Calendar date</strong><small>Show dates like Sep 12</small></span></label><label class="due-date-option"><input type="radio" name="due-date-display" value="countdown" data-due-date-display="countdown" ${!calendarSelected?'checked':''}/><span><strong>Days countdown</strong><small>Show Today, in 2 days, or 1 day overdue</small></span></label></div></section>`;
}
function labelsSettingsSection(){
  const rows=Object.entries(state.labels).map(([name,color])=>`<div class="label-row" data-label-row="${esc(name)}"><input data-label-name value="${esc(name)}" aria-label="Label name"/><input type="color" data-label-color value="${esc(labelColor(name,{[name]:color}))}" aria-label="Label color"/><button type="button" class="secondary" data-update-label>Save</button><button type="button" class="danger" data-delete-label>Delete</button></div>`).join('');
  return `<section class="settings-section labels-settings" data-settings-section="labels" id="settings-labels" tabindex="-1"><div class="section-heading"><div><span class="eyebrow">BOARD</span><h2>Labels</h2><p>${state.boardPath?esc(state.boardTitle):'Open a board to manage its labels.'}</p></div></div><form id="create-label-form" class="label-create"><fieldset ${!state.boardPath?'disabled':''}><label>Name<input id="new-label-name" required maxlength="80" aria-label="New label name"/></label><label>Color<input id="new-label-color" type="color" value="#8b5cf6" aria-label="New label color"/></label><button class="primary" type="submit">Add label</button></fieldset></form><div class="label-list">${rows||'<p class="notice">No labels yet.</p>'}</div></section>`;
}
export async function dialRemotePeer(address:string){if(diagnosticDialQueued||diagnostics.pending)return;diagnosticDialQueued=true;diagnostics.address=address;diagnostics.pending=true;diagnostics.error='';diagnostics.result='';render();try{const value=await invoke<unknown>('dial_remote_peer',{addressJson:address});diagnostics.result=typeof value==='string'?value:JSON.stringify(value); }catch(e){diagnostics.error=message(e);}finally{diagnostics.pending=false;diagnosticDialQueued=false;render();}}
function syncSettingsSection(){return `<section class="settings-section sync-settings" data-settings-section="sync" id="settings-sync" tabindex="-1"><div class="section-heading"><div><span class="eyebrow">IROH</span><h2>Sync</h2><p>Keep this board in step with your other devices.</p></div><span class="status ${state.connected?'online':'offline'}"><i></i>${state.connected?'Connected':'Offline'}</span></div><div class="sync-actions"><div><strong>${state.boardPath?esc(state.boardTitle):'No board open'}</strong><span>${state.connected?'Connected to '+esc(state.peer):esc(state.peer)}</span></div><button class="secondary" id="pair" ${!state.boardPath?'disabled':''}>Pair device</button><button class="primary" id="sync" ${!state.boardPath||state.loading?'disabled':''}>${state.loading?'Syncing…':'Sync now'}</button></div><details class="advanced-settings device-details" open><summary>Device details</summary><div class="device-grid"><div><span>Local endpoint</span><code title="${esc(state.endpointAddress)}">${state.endpointAddress?esc(state.endpointAddress):'Starting…'}</code><button class="text-button" id="copy-endpoint">Copy address</button></div><div><span>Board ID</span><code>${state.boardPath?esc(state.boardId):'Open a board to enable sync'}</code></div></div><div class="remote-diagnostic"><label>Remote endpoint address JSON<textarea id="remote-endpoint" placeholder="Paste endpoint address JSON">${esc(diagnostics.address)}</textarea></label><button class="secondary" id="dial-remote" ${diagnostics.pending?'disabled':''}>${diagnostics.pending?'Dialing…':'Diagnostic dial'}</button>${diagnostics.error?`<p class="error">${esc(diagnostics.error)}</p>`:''}${diagnostics.result?`<pre class="notice">${esc(diagnostics.result)}</pre>`:''}</div></details></section>`;}
function settingsPage(){return `<section class="workspace settings" data-page="settings"><div class="settings-header"><span class="eyebrow">KNOT</span><h1>Settings</h1></div><div class="settings-layout">${hasAI()?modelSettingsSection():''}<div class="board-settings">${dueDateSettingsSection()}${labelsSettingsSection()}${syncSettingsSection()}</div></div></section>`;}


let disposeCardDragging:(()=>void)|null=null;
let suppressColumnClick=false;
function bindColumnDragging(){document.querySelectorAll<HTMLElement>('.column').forEach(column=>{const handle=column.querySelector<HTMLElement>('.column-head');if(!handle)return;handle.addEventListener('pointerdown',downEvent=>{const start=downEvent as PointerEvent;if(start.button!==0||(start.target as HTMLElement).closest('[data-add]'))return;const rect=column.getBoundingClientRect(),offsetX=start.clientX-rect.left,offsetY=start.clientY-rect.top,original=state.columns.map(item=>item.id);let active=false,ghost:HTMLElement|null=null;let cursorX:MotionValue<number>|null=null,cursorY:MotionValue<number>|null=null,tilt:MotionValue<number>|null=null;let stopGhostMotion:(()=>void)|null=null,lastPointerX=start.clientX,tiltTimer:number|null=null;const cleanup=(cancelled:boolean)=>{document.removeEventListener('pointermove',move);document.removeEventListener('pointerup',up);document.removeEventListener('pointercancel',cancel);if(!active)return;column.classList.remove('column-dragging');if(tiltTimer!==null)window.clearTimeout(tiltTimer);stopGhostMotion?.();cursorX?.destroy();cursorY?.destroy();tilt?.destroy();ghost?.remove();suppressColumnClick=true;window.setTimeout(()=>{suppressColumnClick=false;},0);if(!cancelled){const ids=Array.from(document.querySelectorAll<HTMLElement>('.columns > .column')).map(item=>item.dataset.column!);if(ids.some((id,index)=>id!==original[index]))void reorderColumns(ids);}};const move=(event:PointerEvent)=>{if(!active&&Math.hypot(event.clientX-start.clientX,event.clientY-start.clientY)<7)return;if(!active){active=true;column.classList.add('column-dragging');const createdGhost=column.cloneNode(true) as HTMLElement;ghost=createdGhost;createdGhost.classList.remove('column-dragging');createdGhost.classList.add('column-drag-ghost');createdGhost.removeAttribute('data-column');createdGhost.setAttribute('aria-hidden','true');createdGhost.style.width=`${rect.width}px`;createdGhost.style.height=`${Math.min(rect.height,360)}px`;createdGhost.style.transformOrigin=`${offsetX}px ${offsetY}px`;document.body.append(createdGhost);cursorX=motionValue(event.clientX-offsetX);cursorY=motionValue(event.clientY-offsetY);tilt=motionValue(0);const springOptions={stiffness:280,damping:24,mass:.55};const springX=springValue(cursorX,springOptions),springY=springValue(cursorY,springOptions),springTilt=springValue(tilt,{stiffness:220,damping:18,mass:.45});const stopStyle=styleEffect(createdGhost,{x:springX,y:springY,rotate:springTilt});stopGhostMotion=()=>{stopStyle();springX.destroy();springY.destroy();springTilt.destroy();};}cursorX?.set(event.clientX-offsetX);cursorY?.set(event.clientY-offsetY);tilt?.set(Math.max(-7,Math.min(7,(event.clientX-lastPointerX)*.35)));if(tiltTimer!==null)window.clearTimeout(tiltTimer);tiltTimer=window.setTimeout(()=>tilt?.set(0),70);lastPointerX=event.clientX;const target=document.elementFromPoint(event.clientX,event.clientY)?.closest<HTMLElement>('.column');if(target&&target!==column&&target.parentElement===column.parentElement){const targetRect=target.getBoundingClientRect(),before=event.clientX<targetRect.left+targetRect.width/2;target.parentElement!.insertBefore(column,before?target:target.nextElementSibling);}event.preventDefault();};const up=()=>cleanup(false);const cancel=()=>cleanup(true);document.addEventListener('pointermove',move);document.addEventListener('pointerup',up,{once:true});document.addEventListener('pointercancel',cancel,{once:true});});});}
const boardScrollPositions=new Map<string,{left:number;top:number}>();
let renderedBoardPath:string|null=null;
export function render(){
  // State may already point at another board; save the viewport under its old owner.
  const previousScroll=app.querySelector<HTMLElement>('.board-scroll');
  if(previousScroll&&renderedBoardPath)boardScrollPositions.set(renderedBoardPath,{left:previousScroll.scrollLeft,top:previousScroll.scrollTop});
  disposeCardDragging?.();disposeCardDragging=null;
  liveEditor?.destroy();liveEditor=null;syncToast();
  const selectedCard=state.view==='board'&&!state.dialog&&state.selected?state.cards.find(c=>c.id===state.selected):undefined;
  app.innerHTML=`<main>${state.view==='settings'?settingsPage():boardPage()}</main>`+utilityActions()+(selectedCard?editor({...selectedCard,...state.drafts[state.boardPath||'']?.[selectedCard.id]}):'')+(state.conflicts.length?conflictModal(state.conflicts[0]):'')+(state.dialog?appDialog(state.dialog):'')+(state.quickAddOpen?quickAddModal():'')+toast();
  renderedBoardPath=state.view==='board'?state.boardPath:null;
  bind();
  const scroll=app.querySelector<HTMLElement>('.board-scroll');
  const position=renderedBoardPath?boardScrollPositions.get(renderedBoardPath):undefined;
  if(scroll&&position){
    scroll.scrollLeft=position.left;scroll.scrollTop=position.top;
    scroll.dispatchEvent(new Event('scroll'));
  }
  void hydrateCardMarkdown(app);
}
function bind(){
  bindColumnDragging();
  const boardScroll=document.querySelector<HTMLElement>('.workspace[data-page="board"] .board-scroll');
  const boardToolbar=document.querySelector<HTMLElement>('.workspace[data-page="board"] .toolbar');
  if(boardScroll&&boardToolbar){
    const updateBoardFade=()=>{const scrolled=boardScroll.scrollTop>0;boardScroll.classList.toggle('has-scroll',scrolled);boardToolbar.classList.toggle('has-board-scroll',scrolled);};
    boardScroll.addEventListener('scroll',updateBoardFade,{passive:true});
    updateBoardFade();
  }
  document.querySelector('#show-archived')?.addEventListener('click',()=>{state.showArchived=!state.showArchived;render();if(state.showArchived&&!state.archivedCards.length)void loadArchivedCards();});
  document.querySelectorAll<HTMLElement>('[data-restore-archived]').forEach(button=>button.addEventListener('click',event=>{event.stopPropagation();const item=state.archivedCards.find(card=>card.revisionId===button.dataset.restoreArchived);if(item)void restoreArchivedCard(item);}));
  document.querySelector('.toast-dismiss')?.addEventListener('click',()=>{toastMessage='';if(toastTimer!==null)window.clearTimeout(toastTimer);toastTimer=null;render();});
  document.querySelectorAll<HTMLInputElement>('[data-model-filter]').forEach(input=>input.addEventListener('input',()=>{
    const list=document.getElementById(input.dataset.modelFilter!);
    const query=input.value.trim().toLocaleLowerCase();
    let matches=0;
    list?.querySelectorAll<HTMLElement>('.model-row').forEach(row=>{row.hidden=!row.textContent?.toLocaleLowerCase().includes(query);if(!row.hidden)matches++;});
    const empty=list?.nextElementSibling as HTMLElement|null;
    if(empty)empty.hidden=matches>0;
  }));
  document.querySelector('#new-card')?.addEventListener('click',openQuickAdd);
  document.querySelector('#quick-add-cancel')?.addEventListener('click',()=>{state.quickAddOpen=false;render();});
  document.querySelector('#quick-add-input')?.addEventListener('keydown',e=>{const event=e as KeyboardEvent;if(event.key==='Escape'){event.preventDefault();state.quickAddOpen=false;render();}else if(event.key==='Enter'&&event.shiftKey){event.preventDefault();const input=e.currentTarget as HTMLInputElement;void submitQuickAdd(input.value,true);}});
  document.querySelector('#quick-add-form')?.addEventListener('submit',e=>{e.preventDefault();const input=document.querySelector<HTMLInputElement>('#quick-add-input');const submitter=(e as SubmitEvent).submitter as HTMLElement|null;if(input)void submitQuickAdd(input.value,submitter?.dataset.open==='true');});
  document.querySelectorAll<HTMLElement>('[data-add]').forEach(x=>x.addEventListener('click',()=>add(x.dataset.add!)));
  document.querySelector('#open')?.addEventListener('click',()=>openBoard());document.querySelector('#empty-open')?.addEventListener('click',()=>openBoard());
  document.querySelectorAll<HTMLElement>('[data-board-path]').forEach(x=>x.addEventListener('click',()=>switchBoard(x.dataset.boardPath!)));
  document.querySelectorAll<HTMLElement>('[data-view]').forEach(x=>x.addEventListener('click',()=>{state.view=x.dataset.view as View;render();}));
  document.querySelector<HTMLElement>('#rename-board')?.addEventListener('click',e=>beginInlineEdit(e.currentTarget as HTMLElement,state.boardTitle,'inline-title-input','Board name',renameBoard));
  document.querySelector('#add-column')?.addEventListener('click',()=>showDialog({kind:'input',action:'add-column',title:'Add column',label:'Column name',value:''}));
  document.querySelectorAll<HTMLElement>('[data-rename-column]').forEach(x=>x.addEventListener('click',()=>{if(suppressColumnClick){suppressColumnClick=false;return;}const column=state.columns.find(c=>c.id===x.dataset.renameColumn);if(column)beginInlineEdit(x,column.name,'inline-column-input','Column name',next=>renameColumn(column.id,next));}));
  document.querySelector('#copy-endpoint')?.addEventListener('click',async()=>{if(!state.endpointAddress){state.error='Local endpoint address is not ready.';render();return;}try{await navigator.clipboard.writeText(state.endpointAddress);state.error='';}catch(e){state.error='Unable to copy endpoint: '+message(e);}render();});
  document.querySelector('#pair')?.addEventListener('click',()=>showDialog({kind:'input',action:'pair',title:'Pair device',label:'Serialized endpoint address',value:'',multiline:true}));
  document.querySelector<HTMLTextAreaElement>('#remote-endpoint')?.addEventListener('input',event=>{diagnostics.address=(event.currentTarget as HTMLTextAreaElement).value;});
  document.querySelector('#dial-remote')?.addEventListener('click',()=>{const input=document.querySelector<HTMLTextAreaElement>('#remote-endpoint');if(input?.value.trim())void dialRemotePeer(input.value.trim());else{diagnostics.error='Enter endpoint address JSON.';render();}});
  document.querySelector('#sync')?.addEventListener('click',syncNow);
  document.querySelector('#create-label-form')?.addEventListener('submit',event=>{event.preventDefault();const name=document.querySelector<HTMLInputElement>('#new-label-name')?.value.trim()||'';const color=document.querySelector<HTMLInputElement>('#new-label-color')?.value||'#8b5cf6';void createLabel(name,color);});
  document.querySelectorAll<HTMLInputElement>('[data-due-date-display]').forEach(input=>input.addEventListener('change',()=>setDueDateDisplayPreference(input.value as DueDateDisplay)));
  document.querySelectorAll<HTMLElement>('[data-update-label]').forEach(button=>button.addEventListener('click',()=>{const row=button.closest<HTMLElement>('[data-label-row]');if(!row)return;const original=row.dataset.labelRow||'';const name=row.querySelector<HTMLInputElement>('[data-label-name]')?.value.trim()||'';const color=row.querySelector<HTMLInputElement>('[data-label-color]')?.value||'#8b5cf6';void updateLabel(original,name,color);}));
  document.querySelectorAll<HTMLElement>('[data-delete-label]').forEach(button=>button.addEventListener('click',()=>{const row=button.closest<HTMLElement>('[data-label-row]');const name=row?.dataset.labelRow||'';if(name&&window.confirm('Delete this label?'))void deleteLabel(name);}));
  document.querySelectorAll<HTMLElement>('[data-model-provider]').forEach(button=>button.addEventListener('click',()=>{const provider=button.dataset.modelProvider as ModelProvider;if(!providerAvailable(provider))return;if(provider===state.modelSettings.provider)return;state.modelSettings={provider,ollama_url:state.modelSettings.ollama_url,openai_base_url:state.modelSettings.openai_base_url,openai_api_key:state.modelSettings.openai_api_key};state.modelFeedback='';render();if(provider==='ollama')void loadOllamaModels();}));
  document.querySelector<HTMLInputElement>('#keep-model-loaded')?.addEventListener('change',event=>void setKeepModelLoaded((event.currentTarget as HTMLInputElement).checked));
  document.querySelectorAll<HTMLElement>('[data-install-recommended]').forEach(button=>button.addEventListener('click',()=>void downloadHFModel(button.dataset.repo||'',button.dataset.file||'')));
  document.querySelectorAll<HTMLElement>('[data-select-model]').forEach(button=>button.addEventListener('click',()=>void saveModelSettings({provider:'huggingface',model_id:button.dataset.selectModel})));
  document.querySelectorAll<HTMLElement>('[data-select-ollama]').forEach(button=>button.addEventListener('click',()=>void saveModelSettings({provider:'ollama',model_id:button.dataset.selectOllama,ollama_url:document.querySelector<HTMLInputElement>('#ollama-url')?.value.trim()||state.modelSettings.ollama_url||'http://127.0.0.1:11434'})));
  document.querySelectorAll('#refresh-ollama').forEach(button=>button.addEventListener('click',()=>{const url=document.querySelector<HTMLInputElement>('#ollama-url')?.value.trim()||state.modelSettings.ollama_url;void loadOllamaModels(url);}));
  const openaiSettings=()=>({openai_base_url:document.querySelector<HTMLInputElement>('#openai-base-url')?.value.trim()||state.modelSettings.openai_base_url||'https://openrouter.ai/api/v1',openai_api_key:document.querySelector<HTMLInputElement>('#openai-api-key')?.value.trim()||state.modelSettings.openai_api_key});
  document.querySelectorAll('#refresh-openai').forEach(button=>button.addEventListener('click',()=>{const settings=openaiSettings();void loadOpenAIModels(settings.openai_base_url,settings.openai_api_key);}));
  document.querySelector('#check-openai-access')?.addEventListener('click',()=>{const settings=openaiSettings();void checkOpenAIAccess(settings.openai_base_url,settings.openai_api_key);});
  document.querySelectorAll<HTMLElement>('[data-select-openai]').forEach(button=>button.addEventListener('click',()=>void saveModelSettings({provider:'openai',model_id:button.dataset.selectOpenai,...openaiSettings()})));
  document.querySelector('#use-openai-model')?.addEventListener('click',()=>{const model=document.querySelector<HTMLInputElement>('#openai-model')?.value.trim();if(!model){state.error='Enter an OpenAI-compatible model ID.';render();return;}void saveModelSettings({provider:'openai',model_id:model,...openaiSettings()});});
  document.querySelectorAll<HTMLElement>('[data-use-hf]').forEach(button=>button.addEventListener('click',()=>{const repo=document.querySelector<HTMLInputElement>('#hf-repo');const file=document.querySelector<HTMLInputElement>('#hf-file');if(repo)repo.value=button.dataset.useHf||'';file?.focus();}));
  document.querySelector('#delete-all-models')?.addEventListener('click',deleteAllModels);
  document.querySelector('#hf-search-form')?.addEventListener('submit',event=>{event.preventDefault();const query=document.querySelector<HTMLInputElement>('#hf-search')?.value||'';void searchHuggingFace(query);});
  document.querySelector('#download-hf-model')?.addEventListener('click',()=>{const repo=document.querySelector<HTMLInputElement>('#hf-repo')?.value.trim()||'';const file=document.querySelector<HTMLInputElement>('#hf-file')?.value.trim()||'';if(!repo||!file){state.error='Enter a repository and exact .gguf filename.';render();return;}void downloadHFModel(repo,file);});
  const closeCardEditor=()=>{const card=state.selected&&state.cards.find(item=>item.id===state.selected);if(card)void save(card);else{state.selected=null;render();}};
  document.querySelector('#close')?.addEventListener('click',closeCardEditor);
  document.querySelector('.editor')?.addEventListener('keydown',event=>{if((event as KeyboardEvent).key==='Escape'){event.preventDefault();if(document.querySelector('.ce-popover.open'))closeEditorPopovers();else closeCardEditor();}});
  document.querySelectorAll<HTMLElement>('.card-editor [data-pop]').forEach(btn=>btn.addEventListener('click',e=>{e.stopPropagation();const pop=document.getElementById(btn.dataset.pop!);if(!pop)return;const isOpen=pop.classList.contains('open');closeEditorPopovers(pop);if(isOpen){pop.classList.remove('open');btn.setAttribute('aria-expanded','false');}else{placeEditorPopover(btn,pop);pop.classList.add('open');btn.setAttribute('aria-expanded','true');}}));
  document.querySelectorAll<HTMLElement>('.card-editor [data-open-pop]').forEach(item=>item.addEventListener('click',e=>{e.stopPropagation();const pop=document.getElementById(item.dataset.openPop!);const anchor=document.querySelector<HTMLElement>('[data-pop="addPop"]');if(!pop||!anchor)return;closeEditorPopovers(pop);placeEditorPopover(anchor,pop);pop.classList.add('open');}));
  document.querySelectorAll<HTMLElement>('.card-editor [data-toggle-label]').forEach(item=>item.addEventListener('click',e=>{e.stopPropagation();const name=item.dataset.toggleLabel!;const select=document.querySelector<HTMLSelectElement>('#labels');const option=select&&Array.from(select.options).find(o=>o.value===name);if(!option)return;option.selected=!option.selected;item.classList.toggle('selected',option.selected);const cluster=document.querySelector<HTMLElement>('.ce-label-cluster');if(cluster){const existing=Array.from(cluster.querySelectorAll<HTMLElement>('.ce-label-chip')).find(chip=>chip.textContent?.trim()===name);if(option.selected&&!existing){const swatch=item.querySelector<HTMLElement>('.ce-swatch');const chip=document.createElement('button');chip.type='button';chip.className='ce-label-chip';chip.dataset.pop='labelPop';chip.setAttribute('aria-expanded','false');chip.innerHTML=`<span class="ce-label-dot" style="background:${swatch?.style.background||'var(--accent)'}"></span><span></span>`;chip.lastElementChild!.textContent=name;chip.addEventListener('click',ev=>{ev.stopPropagation();const pop=document.getElementById('labelPop')!;const isOpen=pop.classList.contains('open');closeEditorPopovers(pop);if(isOpen){pop.classList.remove('open');}else{placeEditorPopover(chip,pop);pop.classList.add('open');}});cluster.append(chip);}else if(!option.selected)existing?.remove();}if(select)select.dispatchEvent(new Event('change',{bubbles:true}));}));
  document.querySelectorAll<HTMLElement>('.card-editor [data-set-column]').forEach(item=>item.addEventListener('click',()=>{const select=document.querySelector<HTMLSelectElement>('#column');if(select){select.value=item.dataset.setColumn!;select.dispatchEvent(new Event('change',{bubbles:true}));}closeEditorPopovers();}));
  const updateEditorDateText=()=>{const s=document.querySelector<HTMLInputElement>('#start')?.value||'',d=document.querySelector<HTMLInputElement>('#due')?.value||'';const sT=prettyEditorDate(s),dT=prettyEditorDate(d);const target=document.querySelector('#ce-date-text');if(target)target.textContent=sT&&dT?`${sT} → ${dT}`:(sT||dT||'Dates');};
  document.querySelector('#start')?.addEventListener('change',updateEditorDateText);
  document.querySelector('#due')?.addEventListener('change',updateEditorDateText);
  document.querySelector('.card-editor')?.addEventListener('click',e=>{if(!(e.target as HTMLElement).closest('.ce-popover')&&!(e.target as HTMLElement).closest('[data-pop]'))closeEditorPopovers();});
  document.querySelector('.editor-backdrop')?.addEventListener('click',e=>{if(e.target===e.currentTarget)closeCardEditor();});
  const liveParent=document.querySelector<HTMLElement>('#card-live-editor');
  const mdBody=document.querySelector<HTMLTextAreaElement>('#body');
  const titleInput=document.querySelector<HTMLInputElement>('#title');
  const columnSelect=document.querySelector<HTMLSelectElement>('#column');
  const labelsSelect=document.querySelector<HTMLSelectElement>('#labels');
  const dueInput=document.querySelector<HTMLInputElement>('#due');
  const startInput=document.querySelector<HTMLInputElement>('#start');
  if(liveParent&&mdBody&&state.selected){
    const cardId=state.selected,boardPath=state.boardPath||'';
    const updateDraft=(patch:Partial<Card>)=>{
      const drafts=state.drafts[boardPath]??={};
      drafts[cardId]={...drafts[cardId],...patch};
    };
    liveEditor=createLiveEditor(liveParent,{
      value:mdBody.value,
      render:document=>renderObsidianMarkdownBlocks(document,{notes:state.cards.map(note=>note.id===cardId?{...note,body:document}:note),currentId:cardId}),
      hydrate:root=>{void hydrateCardMarkdown(root);},
      onChange:source=>{mdBody.value=source;updateDraft({body:source});},
    });
    mdBody.addEventListener('input',()=>updateDraft({body:mdBody.value}));
    titleInput?.addEventListener('input',()=>updateDraft({title:titleInput.value}));
    columnSelect?.addEventListener('change',()=>updateDraft({column:columnSelect.value}));
    labelsSelect?.addEventListener('change',()=>updateDraft({labels:Array.from(labelsSelect.selectedOptions,opt=>opt.value)}));
    for(const event of ['input','change']){
      dueInput?.addEventListener(event,()=>updateDraft({due:dueInput.value}));
      startInput?.addEventListener(event,()=>updateDraft({start:startInput.value}));
    }
  }
  document.querySelectorAll<HTMLElement>('.card-body.markdown-rendered,#card-live-editor').forEach(root=>root.addEventListener('click',async event=>{
    const link=(event.target as HTMLElement).closest<HTMLAnchorElement>('a');if(!link)return;
    event.stopPropagation();
    if(link.dataset.noteLink!==undefined){event.preventDefault();const note=findNote(link.dataset.noteLink,{notes:state.cards,currentId:state.selected||root.closest<HTMLElement>('[data-id]')?.dataset.id});if(!note){state.error='Note not found in this board: '+link.dataset.noteLink;syncToast();document.querySelector('.toast')?.remove();app.insertAdjacentHTML('beforeend',toast());return;}
      const selected=state.cards.find(card=>card.id===state.selected);if(selected){await save(selected);if(state.selected)return;}state.selected=note.id;render();const fragment=link.dataset.noteLink.split('#')[1];if(fragment){const preview=document.querySelector<HTMLElement>('#card-live-editor');const anchor=Array.from(preview?.querySelectorAll<HTMLElement>('[id]')||[]).find(el=>el.id.endsWith('-'+fragment.toLowerCase().replace(/[^\p{L}\p{N}_-]+/gu,'-')));anchor?.scrollIntoView({block:'start'});}
    }else if(link.getAttribute('href')?.startsWith('#')){event.preventDefault();const id=link.getAttribute('href')!.slice(1);const target=Array.from(root.querySelectorAll<HTMLElement>('[id]')).find(el=>el.id===id);target?.scrollIntoView({block:'nearest'});}
  }));

  document.querySelector('#archive')?.addEventListener('click',async()=>{const card=state.selected&&state.cards.find(item=>item.id===state.selected);if(!card||!state.boardPath)return;try{await invoke<boolean>('delete_card',{path:state.boardPath,id:card.id});state.cards=state.cards.filter(item=>item.id!==card.id);state.selected=null;state.error='Card archived';render();if(state.showArchived)void loadArchivedCards();}catch(e){state.error='Unable to archive card: '+message(e);render();}});
  document.querySelector('#share')?.addEventListener('click',async()=>{const card=state.selected&&state.cards.find(item=>item.id===state.selected);if(!card||!state.boardPath)return;try{const shared=await invoke<string>('share_path',{path:state.boardPath,id:card.id});await navigator.clipboard.writeText(shared);state.error='Card path copied';render();}catch(e){state.error='Unable to copy path: '+message(e);render();}});
  document.querySelectorAll<HTMLInputElement>('[data-label-color],[data-card-label-color]').forEach(input=>input.addEventListener('input',()=>{state.labelColors[input.dataset.labelColor||input.dataset.cardLabelColor||'']=input.value;}));document.querySelector('#save')?.addEventListener('click',()=>{const c=state.cards.find(x=>x.id===state.selected);if(c)save(c);});document.querySelector('#delete')?.addEventListener('click',()=>{if(state.selected)requestDelete(state.selected);});
  document.querySelector('#dialog-cancel')?.addEventListener('click',closeDialog);document.querySelector('.app-dialog')?.addEventListener('keydown',e=>{if((e as KeyboardEvent).key==='Escape')closeDialog();});document.querySelector('#input-dialog-form')?.addEventListener('submit',e=>{e.preventDefault();const value=document.querySelector<HTMLInputElement|HTMLTextAreaElement>('#dialog-input')?.value||'';void submitInputDialog(value);});document.querySelector('#dialog-confirm')?.addEventListener('click',()=>{const dialog=state.dialog;if(dialog?.kind==='confirm-delete'){state.dialog=null;void remove(dialog.cardId);}});
  disposeCardDragging=bindCardDragging(app,{
    open:id=>{state.selected=id;render();},
    canDrag:()=>!cardMovePending,
    move:(id,column,beforeId)=>{void moveCardToSlot(id,column,beforeId);},
    shift:(id,direction)=>{
      const card=state.cards.find(card=>card.id===id);if(!card)return;
      if(direction==='up'||direction==='down'){
        const cards=orderedCards(card.column),index=cards.findIndex(card=>card.id===id);
        if(direction==='up'&&index>0)void moveCardToSlot(id,card.column,cards[index-1].id);
        if(direction==='down'&&index<cards.length-1)void moveCardToSlot(id,card.column,cards[index+2]?.id||null);
      }else{
        const index=state.columns.findIndex(column=>column.id===card.column);
        const column=state.columns[index+(direction==='left'?-1:1)];
        if(column)void moveCardToSlot(id,column.id,null);
      }
    },
  });
  document.querySelectorAll<HTMLElement>('[data-delete]').forEach(x=>x.addEventListener('click',e=>{e.stopPropagation();requestDelete(x.dataset.delete!);}));document.querySelectorAll<HTMLElement>('[data-resolve]').forEach(x=>x.addEventListener('click',()=>resolveConflict(x.dataset.resolve!)));
}

render();
export async function initializeApp(){
  await loadCapabilities();
  if(!capabilitiesLoaded)return;
  if(capabilities.mobile)await openDefaultBoard();
  await loadModels();
  if(!capabilities.mobile)await loadSync();
}
const runningTests=typeof process!=='undefined'&&!!process.env.VITEST;
if(!runningTests){void initializeApp();void import('@tauri-apps/api/event').then(({listen})=>listen<DownloadProgress>('model-download-progress',event=>applyDownloadProgress(event.payload)));setInterval(poll,2000);}
