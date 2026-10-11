// Adds a "Loci" link to the landing page at the end of the menu bar's left buttons,
// just left of the title.
(function () {
    'use strict';
    function addHomeLink() {
        if (document.querySelector('.loci-home')) return;
        var left = document.querySelector('#mdbook-menu-bar .left-buttons') || document.querySelector('.left-buttons');
        if (!left) return;
        var a = document.createElement('a');
        a.className = 'loci-home';
        a.href = '../';
        a.textContent = 'Loci';
        a.title = 'Loci home page';
        left.appendChild(a);
    }
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', addHomeLink);
    } else {
        addHomeLink();
    }
})();
