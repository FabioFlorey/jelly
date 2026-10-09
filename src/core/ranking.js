// Pure page-side ranking rules. Inputs are primitive values or data-only rows;
// DOM access, layout reads, cache mutation and CDP belong to the caller.
(() => {
    const matchQuality = (normalizedName, normalizedQuery) => {
        if (normalizedName === normalizedQuery) return 0;
        if (normalizedName.startsWith(normalizedQuery)) return 1;
        if (normalizedName.includes(normalizedQuery)) return 2;
        return -1;
    };
    const compareActionability = (a, b) =>
        a.disabled - b.disabled || a.offscreen - b.offscreen || a.order - b.order;
    const compareSearch = (a, b) =>
        a.match - b.match || compareActionability(a, b);
    return Object.freeze({matchQuality, compareActionability, compareSearch});
})()
