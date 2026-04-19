// Astro Islands Architecture Simulation
// Mimics Astro's partial hydration with island markers

// Island components with different hydration strategies
const islands = {
  'counter': {
    server: (props) => `<div class="counter-island" data-island="counter" data-count="${props.count || 0}">
  <button class="decrement">-</button>
  <span class="count">${props.count || 0}</span>
  <button class="increment">+</button>
</div>`,
    client: `
      document.querySelectorAll('[data-island="counter"]').forEach(el => {
        let count = parseInt(el.dataset.count);
        const display = el.querySelector('.count');
        el.querySelector('.decrement').onclick = () => { count--; display.textContent = count; };
        el.querySelector('.increment').onclick = () => { count++; display.textContent = count; };
      });
    `,
    strategy: 'load' // hydrate on page load
  },
  'search': {
    server: (props) => `<div class="search-island" data-island="search">
  <input type="text" placeholder="Search..." value="${props.query || ''}" />
  <button>Search</button>
  <div class="results"></div>
</div>`,
    client: `
      document.querySelectorAll('[data-island="search"]').forEach(el => {
        const input = el.querySelector('input');
        const button = el.querySelector('button');
        const results = el.querySelector('.results');
        button.onclick = () => {
          results.innerHTML = '<p>Searching for: ' + input.value + '</p>';
        };
      });
    `,
    strategy: 'idle' // hydrate when browser idle
  },
  'image-carousel': {
    server: (props) => {
      const images = props.images || ['/img1.jpg', '/img2.jpg'];
      return `<div class="carousel-island" data-island="image-carousel" data-images="${JSON.stringify(images).replace(/"/g, '&quot;')}">
  <div class="carousel-container">
    ${images.map((img, i) => `<img src="${img}" ${i === 0 ? '' : 'style="display:none"'} data-index="${i}" />`).join('')}
  </div>
  <button class="prev">←</button>
  <button class="next">→</button>
</div>`;
    },
    client: `
      document.querySelectorAll('[data-island="image-carousel"]').forEach(el => {
        const images = JSON.parse(el.dataset.images);
        let current = 0;
        const imgs = el.querySelectorAll('img');
        el.querySelector('.next').onclick = () => {
          imgs[current].style.display = 'none';
          current = (current + 1) % images.length;
          imgs[current].style.display = 'block';
        };
        el.querySelector('.prev').onclick = () => {
          imgs[current].style.display = 'none';
          current = (current - 1 + images.length) % images.length;
          imgs[current].style.display = 'block';
        };
      });
    `,
    strategy: 'visible' // hydrate when visible (simplified to 'load' for test)
  }
};

// Pages with island components
const pages = {
  '/': {
    title: 'Astro Islands Demo',
    islands: [
      { name: 'counter', props: { count: 5 }, hydration: 'load' },
      { name: 'search', props: {}, hydration: 'idle' }
    ]
  },
  '/gallery': {
    title: 'Image Gallery',
    islands: [
      { name: 'image-carousel', props: { images: ['/photo1.jpg', '/photo2.jpg', '/photo3.jpg'] }, hydration: 'visible' }
    ]
  }
};

function renderPage(path, page) {
  const islandHtml = page.islands.map(island => {
    const component = islands[island.name];
    return component.server(island.props);
  }).join('\n');
  
  const islandScripts = page.islands.map(island => {
    const component = islands[island.name];
    return `// ${island.name} island (${island.hydration})\n${component.client}`;
  }).join('\n\n');
  
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${page.title} - Astro on NANO</title>
  <style>
    body { font-family: system-ui, sans-serif; max-width: 900px; margin: 2rem auto; padding: 0 1rem; }
    .counter-island { display: flex; align-items: center; gap: 1rem; padding: 1rem; border: 1px solid #ddd; border-radius: 8px; }
    .counter-island button { padding: 0.5rem 1rem; font-size: 1.2rem; cursor: pointer; }
    .count { font-size: 1.5rem; font-weight: bold; min-width: 3ch; text-align: center; }
    .search-island { margin: 1rem 0; }
    .search-island input { padding: 0.5rem; width: 300px; }
    .carousel-island { margin: 2rem 0; }
    .carousel-container img { max-width: 100%; border-radius: 8px; }
    .astro-badge { display: inline-block; background: #ff5d01; color: white; padding: 0.25rem 0.5rem; border-radius: 4px; font-size: 0.8rem; }
  </style>
</head>
<body>
  <header>
    <span class="astro-badge">Astro</span>
    <h1>${page.title}</h1>
    <nav>
      <a href="/">Home</a> |
      <a href="/gallery">Gallery</a>
    </nav>
  </header>
  
  <main>
    ${islandHtml}
  </main>
  
  <footer>
    <p>Server-rendered on NANO • Islands architecture demo</p>
  </footer>
  
  <script>
    console.log('[Astro] Hydrating islands...');
    ${islandScripts}
    console.log('[Astro] Islands hydrated');
  </script>
</body>
</html>`;
}

export default {
  async fetch(request) {
    const url = new URL(request.url);
    const pathname = url.pathname;
    
    console.log(`[Astro Islands] ${request.method} ${pathname}`);
    
    // Handle image assets
    if (pathname.startsWith('/photo') && pathname.endsWith('.jpg')) {
      // Return a placeholder response for images
      return new Response('Placeholder image data', {
        status: 200,
        headers: { 
          'Content-Type': 'image/jpeg',
          'X-Astro-Asset': 'true'
        }
      });
    }
    
    // Serve pages
    const page = pages[pathname];
    if (page) {
      const html = renderPage(pathname, page);
      return new Response(html, {
        status: 200,
        headers: { 
          'Content-Type': 'text/html; charset=utf-8',
          'X-Astro-Islands': 'true',
          'Cache-Control': 'public, max-age=60'
        }
      });
    }
    
    // 404
    return new Response(`<!DOCTYPE html>
<html>
<head><title>404 - Not Found</title></head>
<body>
  <h1>Page not found</h1>
  <p>No Astro page at <code>${pathname}</code></p>
  <p>Available: ${Object.keys(pages).join(', ')}</p>
  <a href="/">Go home</a>
</body>
</html>`, {
      status: 404,
      headers: { 'Content-Type': 'text/html; charset=utf-8' }
    });
  }
};
