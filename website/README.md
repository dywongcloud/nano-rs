# NANO Website

Professional website for nano-rs runtime using Tailwind CSS.

## Structure

```
website/
├── index.html          # Main page with all sections
├── css/                # Styles (if needed beyond Tailwind)
├── js/                 # JavaScript interactions
└── README.md           # This file
```

## Development

### Local Development

Since this uses Tailwind CSS via CDN, you can serve it with any static file server:

```bash
# Using Python 3
python -m http.server 8080

# Using Node.js (npx serve)
npx serve .

# Using PHP
php -S localhost:8080
```

Then open http://localhost:8080

### Deployment

The website can be deployed to any static hosting:

- GitHub Pages
- Netlify
- Vercel
- AWS S3 + CloudFront
- Any web server (nginx, Apache, etc.)

## Features

- **Tailwind CSS** - Modern utility-first CSS via CDN
- **Responsive** - Mobile-first design
- **Dark Theme** - Professional dark mode
- **Syntax Highlighting** - Code blocks with highlighting
- **SVG Architecture Diagram** - Visual system overview
- **Smooth Scrolling** - Navigation with anchor links
- **No Build Step** - Works directly in browser

## Content Sections

1. **Hero** - Value proposition with stats
2. **Features** - 6 feature cards with icons
3. **Quick Start** - Installation and usage guide
4. **Compatibility Matrix** - API support tables
5. **Footer** - Links and resources

## Customization

### Colors
Edit the Tailwind config in the HTML:
```javascript
tailwind.config = {
    theme: {
        extend: {
            colors: {
                nano: {
                    500: '#3b82f6',
                    600: '#2563eb',
                },
            },
        },
    },
}
```

### Content
Edit the HTML directly to update:
- Hero section text
- Feature descriptions
- Code examples
- Links and URLs

## Browser Support

- Chrome/Edge 90+
- Firefox 90+
- Safari 14+
- Modern mobile browsers

## Performance

- Tailwind CSS via CDN (cached)
- Google Fonts via CDN (cached)
- No JavaScript required for basic functionality
- Minimal custom CSS
- Optimized for fast loading

## License

Same as nano-rs project - MIT License.
