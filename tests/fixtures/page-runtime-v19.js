(() => {
    const key = '__jellyRuntimeV1';
    const version = 19;
    if (globalThis[key]?.version === version) return true;
    globalThis[key]?.dispose?.();
    document.querySelectorAll('[data-jelly-ref]').forEach(e => {
        if (/^e\d+$/.test(e.getAttribute('data-jelly-ref') || '')) {
            e.removeAttribute('data-jelly-ref');
        }
    });

    const interactiveRoles = new Set(['button','link','textbox','checkbox','radio','combobox','option','tab','menuitem','switch','slider','spinbutton','treeitem']);
    const selector = 'a,button,input,textarea,select,[role],[tabindex],[contenteditable=true],[draggable=true],[onclick]';
    const normalize = value => (value || '').replace(/\s+/g, ' ').trim();
    const isInteractive = element => {
    const role = element.getAttribute('role');
    return ['A','BUTTON','INPUT','TEXTAREA','SELECT'].includes(element.tagName) ||
        interactiveRoles.has(role) ||
        element.tabIndex >= 0 ||
        element.isContentEditable ||
        element.draggable ||
        !!element.onclick;
};
    const measure = element => {
    if (!element?.isConnected) return null;
    const rect = element.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const style = getComputedStyle(element);
    if (
        style.display === 'none' ||
        style.visibility === 'hidden' ||
        style.opacity === '0'
    ) return null;
    return rect;
};
    const inferRole = element => element.getAttribute('role') || ({
    A:'link',
    BUTTON:'button',
    INPUT:(element.type === 'checkbox'
        ? 'checkbox'
        : element.type === 'radio'
            ? 'radio'
            : 'textbox'),
    TEXTAREA:'textbox',
    SELECT:'combobox'
})[element.tagName] || (element.draggable ? 'draggable' : '');
    const inViewport = rect =>
    rect.bottom > 0 &&
    rect.right > 0 &&
    rect.top < innerHeight &&
    rect.left < innerWidth;
    const collectRoots = () => {
        const roots = [document];
        const seen = new Set(roots);
        for (let i = 0; i < roots.length; i++) {
            for (const e of roots[i].querySelectorAll('*')) {
                const shadow = e.shadowRoot;
                if (shadow && !seen.has(shadow)) {
                    seen.add(shadow);
                    roots.push(shadow);
                }
            }
        }
        return roots;
    };
    const labelledByText = e => {
        const root = e.getRootNode();
        return normalize(
            (e.getAttribute('aria-labelledby') || '')
                .split(/\s+/)
                .filter(Boolean)
                .map(id => root.getElementById?.(id)?.textContent || document.getElementById(id)?.textContent || '')
                .join(' ')
        );
    };
    const slottedText = e => {
        if (!e?.querySelectorAll) return '';
        const parts = [];
        for (const slot of e.querySelectorAll('slot')) {
            for (const node of slot.assignedNodes({flatten:true})) {
                const text = normalize(node.textContent || '');
                if (text) parts.push(text);
            }
        }
        return normalize(parts.join(' '));
    };
    const semanticNameInfo = (e, labelByFor) => {
        const formControl = ['INPUT','TEXTAREA','SELECT'].includes(e.tagName);
        const explicitLabel = formControl && e.id ? labelByFor.get(e.id) || '' : '';
        const nestedLabel = formControl ? e.closest('label')?.textContent || '' : '';
        const siblingLabel = formControl
            ? (e.nextElementSibling?.tagName === 'LABEL' ? e.nextElementSibling.textContent :
               e.previousElementSibling?.tagName === 'LABEL' ? e.previousElementSibling.textContent : '')
            : '';
        const stable = normalize(
            e.getAttribute('aria-label') ||
            labelledByText(e) ||
            explicitLabel ||
            nestedLabel ||
            siblingLabel ||
            normalize(e.textContent) ||
            slottedText(e)
        );
        if (stable) return {name: stable, valueSensitive: false};
        return {
            name: normalize(e.value || e.placeholder || e.alt || e.getAttribute('title') || ''),
            valueSensitive: 'value' in e
        };
    };
    const semanticName = (e, labelByFor) => semanticNameInfo(e, labelByFor).name;
    const observerOptions = {
        subtree: true,
        childList: true,
        characterData: true,
        attributes: true,
        attributeFilter: [
            'aria-label','aria-labelledby','role','tabindex','href','contenteditable','draggable','onclick',
            'value','placeholder','alt','title','for'
        ]
    };

    let documentToken;
    try {
        const words = crypto.getRandomValues(new Uint32Array(2));
        documentToken = 'e' + words[0].toString(36) + words[1].toString(36).padStart(7, '0');
    } catch {
        documentToken = 'e' + Date.now().toString(36) + Math.floor(performance.now() * 1000).toString(36);
    }

    const runtime = {
        version,
        documentToken,
        epoch: 0,
        dirty: true,
        nextRef: 0,
        elementRefs: new WeakMap(),
        refs: new Map(),
        items: new WeakMap(),
        names: new Map(),
        labelsByRoot: new Map(),
        cache: [],
        observer: null,
        onInput: null,
        nativeAttachShadow: Element.prototype.attachShadow,
        attachShadowHook: null,
        observedRoots: 0,
        metrics: {
            genericTextFallbacks: 0,
            genericTextPrefilterHits: 0,
            genericTextSlowFallbacks: 0,
            genericTextFallbackMisses: 0
        },

        markDirty() {
            if (this.dirty) return;
            this.dirty = true;
            this.epoch++;
        },

        observeRoots(roots) {
            this.observer?.disconnect();
            for (const root of roots) {
                const node = root === document ? document.documentElement : root;
                if (node) this.observer?.observe(node, observerOptions);
            }
            this.observedRoots = roots.length;
        },

        dispose() {
            this.observer?.disconnect();
            if (this.onInput) {
                document.removeEventListener('input', this.onInput, true);
                document.removeEventListener('change', this.onInput, true);
            }
            if (this.attachShadowHook && Element.prototype.attachShadow === this.attachShadowHook) {
                Element.prototype.attachShadow = this.nativeAttachShadow;
            }
        },

        rebuild() {
            const t0 = performance.now();
            const roots = collectRoots();
            const tRoots = performance.now();
            const selected = roots.flatMap(root => [...root.querySelectorAll(selector)]);
            const t1 = performance.now();
            const candidates = selected.filter(isInteractive);
            const t2 = performance.now();
            const refs = new Map();
            const items = new WeakMap();
            const names = new Map();
            const cache = [];

            for (const e of candidates) {
                let ref = this.elementRefs.get(e);
                if (!ref) {
                    ref = this.documentToken + '-' + (++this.nextRef);
                    this.elementRefs.set(e, ref);
                }
                refs.set(ref, e);
                const item = {e, ref, name:'', valueSensitive:false, root:e.getRootNode(), order:cache.length};
                items.set(e, item);
                cache.push(item);
            }
            const t3 = performance.now();

            const labelsByRoot = new Map(
                roots.map(root => [
                    root,
                    new Map(
                        [...root.querySelectorAll('label[for]')]
                            .map(label => [label.htmlFor, normalize(label.textContent)])
                            .filter(([forId]) => !!forId)
                    )
                ])
            );
            for (const item of cache) {
                const info = semanticNameInfo(item.e, labelsByRoot.get(item.root) || new Map());
                item.name = info.name;
                item.valueSensitive = info.valueSensitive;
            }
            const t4 = performance.now();

            for (const item of cache) {
                const normalizedName = normalize(item.name);
                if (!normalizedName) continue;
                const matches = names.get(normalizedName) || [];
                matches.push(item.e);
                names.set(normalizedName, matches);
            }
            const t5 = performance.now();

            this.refs = refs;
            this.items = items;
            this.names = names;
            this.labelsByRoot = labelsByRoot;
            this.cache = cache;
            this.dirty = false;
            this.observeRoots(roots);
            this.lastRebuildTimings = {
                roots: roots.length,
                selected: selected.length,
                candidates: candidates.length,
                roots_ms: tRoots - t0,
                query_ms: t1 - tRoots,
                filter_ms: t2 - t1,
                refs_ms: t3 - t2,
                names_ms: t4 - t3,
                index_ms: t5 - t4,
                total_ms: t5 - t0
            };
        },

        ensure() {
            if (this.dirty) this.rebuild();
        },

        resolveRef(ref) {
            this.ensure();
            const e = this.refs.get(ref);
            return e?.isConnected ? e : null;
        },

        currentName(item) {
            const e = item?.e;
            if (!e) return '';
            if (item.valueSensitive) {
                return semanticName(e, this.labelsByRoot.get(item.root) || new Map());
            }
            return item.name;
        },

        resolveText(text) {
            this.ensure();
            const q = normalize(text);
            const candidates = [];
            const seen = new Set();

            const add = item => {
                if (!item || seen.has(item.e)) return;
                if (normalize(this.currentName(item)) !== q) return;
                const described = this.describe(item);
                if (!described) return;
                seen.add(item.e);
                candidates.push({
                    disabled: described.disabled ? 1 : 0,
                    offscreen: described.in_viewport ? 0 : 1,
                    order:item.order,
                    e:item.e
                });
            };

            for (const e of this.names.get(q) || []) add(this.items.get(e));
            for (const item of this.cache) {
                if (item.valueSensitive) add(item);
            }

            candidates.sort((a, b) => a.disabled - b.disabled || a.offscreen - b.offscreen || a.order - b.order);
            return candidates[0]?.e || null;
        },

        describe(item) {
            const e = item.e;
            const r = measure(e);
            if (!r) return null;
            const role = inferRole(e);
            const ariaDisabled = e.getAttribute('aria-disabled');
            const ariaChecked = e.getAttribute('aria-checked');
            const checked = 'checked' in e
                ? !!e.checked
                : ariaChecked === 'true'
                    ? true
                    : ariaChecked === 'false'
                        ? false
                        : null;
            return {
                ref:'@' + item.ref,
                tag:e.tagName.toLowerCase(),
                role,
                name:this.currentName(item).slice(0, 160),
                disabled:!!e.disabled || ariaDisabled === 'true',
                checked,
                draggable:!!e.draggable,
                shadow:e.getRootNode() instanceof ShadowRoot,
                in_viewport:inViewport(r),
                x:Math.round(r.x),
                y:Math.round(r.y),
                width:Math.round(r.width),
                height:Math.round(r.height)
            };
        },

        snapshot(limit = 0, offset = 0) {
            this.ensure();
            const out = [];
            let skipped = 0;
            for (const item of this.cache) {
                const described = this.describe(item);
                if (!described) continue;
                if (skipped < offset) {
                    skipped++;
                    continue;
                }
                out.push(described);
                if (limit > 0 && out.length >= limit) break;
            }
            return out;
        },

        search(query, limit = 30, offset = 0) {
            this.ensure();
            const q = normalize(query).toLowerCase();
            const ranked = [];
            for (let order = 0; order < this.cache.length; order++) {
                const item = this.cache[order];
                const name = normalize(this.currentName(item)).toLowerCase();
                if (!name) continue;
                let match = 99;
                if (name === q) match = 0;
                else if (name.startsWith(q)) match = 1;
                else if (name.includes(q)) match = 2;
                else continue;
                const described = this.describe(item);
                if (!described) continue;
                ranked.push({
                    match,
                    disabled: described.disabled ? 1 : 0,
                    offscreen: described.in_viewport ? 0 : 1,
                    order,
                    value:described
                });
            }
            ranked.sort((a, b) =>
                a.match - b.match ||
                a.disabled - b.disabled ||
                a.offscreen - b.offscreen ||
                a.order - b.order
            );
            const start = Math.max(0, offset);
            return ranked.slice(start, start + Math.max(1, limit)).map(entry => entry.value);
        }
    };

    runtime.observer = new MutationObserver(() => runtime.markDirty());
    runtime.observeRoots([document]);
    runtime.attachShadowHook = function attachShadow(init) {
        const root = runtime.nativeAttachShadow.call(this, init);
        if (init?.mode === 'open') {
            runtime.observer?.observe(root, observerOptions);
            runtime.markDirty();
        }
        return root;
    };
    Element.prototype.attachShadow = runtime.attachShadowHook;
    runtime.onInput = () => runtime.markDirty();
    document.addEventListener('input', runtime.onInput, true);
    document.addEventListener('change', runtime.onInput, true);

    Object.defineProperty(globalThis, key, {
        value: runtime,
        configurable: true,
        writable: true,
        enumerable: false
    });
    return true;
})()