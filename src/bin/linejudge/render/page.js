document.querySelectorAll('.chip.pick').forEach(chip => {
  chip.addEventListener('click', () => {
    const group = chip.dataset.group;
    document.querySelectorAll('.dv[data-group="' + group + '"]').forEach(shown => {
      shown.hidden = shown.dataset.value !== chip.dataset.value;
    });
    document.querySelectorAll('.chip.pick[data-group="' + group + '"]').forEach(one => {
      one.classList.toggle('active', one === chip);
    });
  });
});
