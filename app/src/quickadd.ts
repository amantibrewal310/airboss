import { mount } from 'svelte';
import QuickAdd from './windows/quickadd/QuickAdd.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('missing #app');
mount(QuickAdd, { target });
