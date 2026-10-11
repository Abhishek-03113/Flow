// Flow landing page: theme-aware product clips, scroll-driven motion, reveal on scroll,
// and the "Get Flow" link (latest GitHub release, or the repository when there is none).
(() => {
  const REPO = 'Abhishek-03113/Flow';
  const REPO_URL = `https://github.com/${REPO}`;
  const root = document.documentElement;
  root.classList.add('js');

  const dark = window.matchMedia('(prefers-color-scheme: dark)');
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');

  /* ── Year in the credit footer ── */
  document.querySelectorAll('[data-year]').forEach((el) => {
    el.textContent = String(new Date().getFullYear());
  });

  /* ── Get Flow: latest release if one exists, otherwise the repository ── */
  const getLinks = document.querySelectorAll('[data-get]');
  const note = document.querySelector('[data-release-note]');
  getLinks.forEach((a) => (a.href = REPO_URL));
  fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
    headers: { Accept: 'application/vnd.github+json' },
  })
    .then((r) => (r.ok ? r.json() : null))
    .then((release) => {
      if (!release || !release.html_url) {
        if (note) note.textContent = 'Opens the Flow project on GitHub.';
        return;
      }
      getLinks.forEach((a) => (a.href = release.html_url));
      if (note) note.textContent = `Latest version: ${release.name || release.tag_name}`;
    })
    .catch(() => {
      /* Offline or rate limited: the repository link stays. */
    });

  /* ── Product clips follow the system appearance ── */
  const videos = Array.from(document.querySelectorAll('video.themed'));
  const theme = () => (dark.matches ? 'dark' : 'light');
  function applyTheme() {
    const t = theme();
    videos.forEach((v) => {
      const src = v.dataset.src.replace('{t}', t);
      if (v.getAttribute('src') === src) return;
      const wasPlaying = !v.paused;
      v.poster = v.dataset.poster.replace('{t}', t);
      v.src = src;
      if (wasPlaying) v.play().catch(() => {});
    });
  }
  applyTheme();
  dark.addEventListener('change', applyTheme);

  if (reduced.matches) videos.forEach((v) => (v.controls = true));

  /* ── Play clips only while they are on screen ── */
  const videoObserver = new IntersectionObserver(
    (entries) => {
      entries.forEach(({ target, isIntersecting }) => {
        if (reduced.matches) return;
        if (isIntersecting) {
          target.play().catch(() => {});
        } else {
          target.pause();
        }
      });
    },
    { threshold: 0.35 },
  );
  videos.forEach((v) => videoObserver.observe(v));

  /* ── Reveal on scroll, with a gentle stagger for siblings ── */
  const revealObserver = new IntersectionObserver(
    (entries) => {
      entries.forEach(({ target, isIntersecting }) => {
        if (!isIntersecting) return;
        target.classList.add('is-visible');
        revealObserver.unobserve(target);
      });
    },
    { threshold: 0.15, rootMargin: '0px 0px -40px 0px' },
  );
  document.querySelectorAll('.reveal').forEach((el) => {
    const siblings = Array.from(el.parentElement.children).filter((c) => c.classList.contains('reveal'));
    el.style.setProperty('--delay', `${Math.min(siblings.indexOf(el), 6) * 0.08}s`);
    revealObserver.observe(el);
  });

  /* ── Hero desk animation runs only while visible ── */
  const desk = document.querySelector('.desk');
  if (desk) {
    new IntersectionObserver(([e]) => desk.classList.toggle('is-playing', e.isIntersecting && !reduced.matches), {
      threshold: 0.2,
    }).observe(desk);
  }

  /* ── Scroll-driven sections publish their progress as --p (0 → 1) ── */
  const nav = document.querySelector('[data-nav]');
  const showcase = document.querySelector('.showcase');
  const declutter = document.querySelector('.declutter');
  const clamp = (n) => Math.min(1, Math.max(0, n));

  function onScroll() {
    const vh = window.innerHeight;
    if (nav) nav.classList.toggle('is-scrolled', window.scrollY > 8);
    if (reduced.matches) {
      showcase?.style.setProperty('--p', '1');
      declutter?.style.setProperty('--p', '1');
      return;
    }
    if (showcase) {
      const r = showcase.getBoundingClientRect();
      // 0 when the window peeks in at the bottom, 1 once it sits in the upper third.
      showcase.style.setProperty('--p', clamp((vh - r.top) / (vh * 0.85)).toFixed(4));
    }
    if (declutter) {
      const r = declutter.getBoundingClientRect();
      declutter.style.setProperty('--p', clamp(-r.top / (r.height - vh)).toFixed(4));
    }
  }

  let ticking = false;
  const schedule = () => {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(() => {
      ticking = false;
      onScroll();
    });
  };
  window.addEventListener('scroll', schedule, { passive: true });
  window.addEventListener('resize', schedule);
  reduced.addEventListener('change', schedule);
  onScroll();
})();
