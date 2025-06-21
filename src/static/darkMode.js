function toggleDarkMode() {
    const htmlElement = document.documentElement;
    htmlElement.classList.toggle('dark');

    // Save user preference to localStorage
    const isDark = htmlElement.classList.contains('dark');
    localStorage.setItem('theme', isDark ? 'dark' : 'light');
}

// On page load, apply saved theme
document.addEventListener('DOMContentLoaded', () => {
    const savedTheme = localStorage.getItem('theme');
    if (savedTheme === 'dark') {
        document.documentElement.classList.add('dark');
    }
});
