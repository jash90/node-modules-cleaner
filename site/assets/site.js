// Gallery lightbox. Without JavaScript each thumbnail is a plain link to the full image.
(() => {
  const dialog = document.querySelector('.lightbox');
  if (!dialog || typeof dialog.showModal !== 'function') return;

  const image = dialog.querySelector('img');
  const caption = dialog.querySelector('.lightbox-caption');
  const close = dialog.querySelector('.lightbox-close');
  let opener = null;

  document.querySelectorAll('[data-lightbox]').forEach((link) => {
    link.addEventListener('click', (event) => {
      event.preventDefault();
      const thumbnail = link.querySelector('img');
      opener = link;
      image.src = link.href;
      image.alt = thumbnail ? thumbnail.alt : '';
      caption.textContent = link.dataset.caption || '';
      dialog.showModal();
      close.focus();
    });
  });

  close.addEventListener('click', () => dialog.close());
  // A click on the backdrop lands on the dialog element itself.
  dialog.addEventListener('click', (event) => {
    if (event.target === dialog) dialog.close();
  });
  dialog.addEventListener('close', () => {
    image.removeAttribute('src');
    if (opener) opener.focus();
  });
})();
