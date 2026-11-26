# Migración de escritorio a Iced + wgpu

Estado al 22 de noviembre de 2026, sobre el árbol local de 1.6.0 (base `86a6c52`). Sin commits.

## Estado

**Iced es la única interfaz desde la versión 1.7 (26 de noviembre de 2026).** Fue la
predeterminada desde el 22 de noviembre, por decisión del propietario tras la comparación en
pantalla con GPUI, y en 1.7 se retiró GPUI (`src/ui/`, la característica `ui-gpui` y su
dependencia); su código queda en el historial de git hasta la 1.6.

Aceptar las diferencias no equivale a haberlas probado: los flujos con cuenta de TIDAL y
el resto de «Sin verificar» no se han ejecutado con Iced y conviene recorrerlos antes de
publicar.

La comparación cubrió Biblioteca, álbum desplegado, menú de pista, cola, letras en ambas
vistas y Ajustes, con el mismo perfil local, contenido y tamaño de ventana.

CI prueba las dos interfaces. `scripts/package.ps1` empaqueta la predeterminada, Iced.

## Compilar

| Variante | Orden |
| --- | --- |
| Iced (predeterminada) | `cargo build --release --locked` |
| GPUI (referencia) | `cargo build --release --locked --no-default-features --features ui-gpui,kanji-romaji` |
| Ambas (abre Iced; `--ui=gpui` o `ABYSS_UI=gpui` abre GPUI) | `cargo build --release --locked --features ui-gpui` |

Versiones fijadas: `iced` 0.14.0 y `wgpu` 27.0.1. La compatibilidad se comprobó en docs.rs
(`iced_wgpu` 0.14.0 requiere `wgpu ^27.0`) y en el changelog oficial de Iced 0.14.0
(wgpu 27, cosmic-text 0.15). `naga` figura como dependencia solo para que compilen ambas
interfaces juntas (`termcolor` compartido con naga 25 de GPUI).

`build.rs` incrusta `assets/abyss.manifest` (DPI por monitor y controles comunes 6) cuando
no se compila GPUI, que trae el suyo.

## Estructura

- `src/iced_ui/`: interfaz Iced. `gpu/lyrics.rs` y `gpu/backdrop.rs` son los únicos
  renderizadores propios. `glide.rs` da inercia a la rueda.
- `src/shared/` y `src/art.rs`: lógica e iconos que usan las dos interfaces.
- Motor de audio, WASAPI, persistencia, TIDAL y Last.fm: los mismos módulos.

Letras: cada fila se rasteriza una vez a escala física en un atlas. Se generan tres niveles
de desenfoque y el shader interpola entre ellos. El layout y la textura se reutilizan
mientras no cambien documento, ancho ni escala.

En Windows, Iced usa DirectX 12 salvo que se defina `WGPU_BACKEND`.

## Regresiones halladas al comparar con GPUI

| Regresión en Iced | Corrección |
| --- | --- |
| Reproducir con la Biblioteca filtrada dejaba en cola solo las filas visibles | La cola es la Biblioteca entera y las filas conservan su número |
| Desenfoque de letras unas cinco veces mayor que la referencia y halo más duro | Sigma de 0,028 del tamaño de letra y halo gaussiano |
| Rueda a saltos, con un tercio menos de fotogramas que GPUI | Inercia de 99 px por muesca y 75 ms, como GPUI |
| 1,04 % de CPU en reposo, frente a 0,10 % | Sondeo cada segundo sin actividad: 0,10–0,47 % |
| Cabecera de Biblioteca y barra superior distintas | Título por sección, barra de herramientas, marca y orden de botones |
| Álbum desplegado en una columna | Hasta cuatro columnas con número, título y duración |
| Menú de pista local con otro orden y una opción de borrado que GPUI no tiene | Igual que GPUI |
| Ajustes sin resumen por sección, calidades y escalas del ecualizador distintas | Resumen, mismo orden y mismas escalas |
| La flecha del menú de orden salía al revés que la de la cabecera | Las mismas marcas que GPUI |
| Ajustes se aplastaba en ventana estrecha con panel lateral | Por debajo de 900 px las secciones pasan arriba, como en GPUI |
| Títulos largos de tarjetas y filas se pisaban (Iced 0.14 no recorta con «…») | Widget `ellipsis::Line` que mide y corta con «…»; lo usa `widgets::line` |
| Barra lateral sin panel, sin nombre, secciones en mayúsculas, sin listas en el carril | Panel con margen y borde, marca y nombre, punto de acento, listas con portada, ficha de cuenta compacta |
| Cabecera de álbum, lista, mix y artista sin panel ni acciones con texto | Panel de cristal de GPUI, título de 40 px, artista enlazado, duración h:mm:ss, acciones «Añadido · Compartir · Más» |
| Filas de pistas con «···» siempre visible y artista apagado | «···» solo al pasar; artista, álbum y tiempos en semibold como GPUI |
| Estanterías de álbum y artista sin panel | Panel plano con título de 16 px |
| Home con pestañas y filas recientes grandes | Sin pestañas (Staff picks y Uploads están en Mixes); filas de 40 px con flecha, colección con fondo y botón de reproducir |
| Ajustes: cabeceras, opciones de calidad, salida y resumen distintos | Icono en recuadro, círculos de calidad, píldora de salida, chips de estado |
| Títulos de página de 28 px y 4 px más arriba | 30 px (36 en Home) a 22 px del borde, como GPUI |
| Texto más fino y borroso: la cobertura de cada glifo se usaba como opacidad sin corregir | `vendor/cryoglyph` aplica la corrección gamma (1,8) y de contraste (1,0) de DirectWrite, la misma que GPUI en `alpha_correction.hlsl` |
| Portadas algo suaves: tamaño lógico fraccionario, la GPU las reescalaba un píxel | `images::snap` ajusta el tamaño a píxeles físicos enteros: textura y pantalla 1:1 |
| Hover instantáneo en filas, barra lateral, Home y botones de icono | Widget `wash`: fundido de 143 ms con suavizado y franja de acento en las filas, como `interact::hover` |
| Fundido de página y paneles con un temporizador fijo de 8,3 ms, desacompasado del refresco | Fotogramas en cada vsync (`request_redraw`) y la página entra deslizándose 26 px desde su lado |
| Pie de tarjetas más grande (13,5 / 12 / 11 px) | 12 / 11,5 / 10 px en negrita y huecos de 7 y 3 px, como GPUI |

## Pruebas

| Comprobación | Resultado |
| --- | --- |
| Pruebas, Iced | 206 pasan |
| Pruebas, GPUI | 193 pasan (referencia: 190, ninguna perdida) |
| Pruebas, ambas interfaces juntas | 210 pasan |
| Clippy, GPUI | sin avisos nuevos frente a la referencia |
| Clippy, Iced | 5 avisos, todos existentes en el código de 1.6.0 |
| `cargo fmt --check`, `git diff --check` | limpios |
| Producción Windows | Iced, GPUI y la orden de `scripts/package.ps1` compilan |
| Sin diccionario japonés | compila (36,3 MiB; con diccionario, 81,9 MiB) |
| Manifiesto de Windows | presente, una sola vez, en Iced, GPUI y ambas juntas |

Recorrido de aceptación en pantalla el 22 de noviembre de 2026, completo sobre el ejecutable
definitivo (SHA-256 `59ce49744610cac7b69ed21c99e2fc4762c544a86c7c4c04031a6af5281522e0`),
con perfiles aislados y una pista FLAC generada para las ediciones:

| Flujo | Resultado |
| --- | --- |
| FLAC y LRC en panel y pantalla completa, seguimiento, desplazamiento manual y Sincronizar | correcto |
| Saltos encadenados, salto al tocar una línea, pista siguiente y anterior | correcto |
| Cola al filtrar, cola de 1.503, aleatorio, quitar una fila y volver al orden | correcto |
| Cierre y restauración (pista 1.502, segundo 27,9) | correcto |
| Puntos de carga en ambas vistas, movimiento reducido (`ABYSS_REDUCED_MOTION=1`) | correcto |
| Menú de orden, álbum desplegado | correcto |
| Ecualizador paramétrico: añadir y arrastrar un filtro, medidores | correcto |
| Editor de etiquetas: guardar y restaurar idéntico byte a byte | correcto |
| Selector de archivos: abrir y cancelar | correcto |
| Ctrl + rueda (dos pasos, 125 %) y Ctrl+0 | correcto |
| Tema claro, cambio de idioma a inglés y vuelta | correcto |
| Ventana mínima, tirador del panel, maximizar y restaurar con doble clic | correcto |
| Perfil de GPUI abierto en Iced y de Iced abierto en GPUI | correcto |

En la compilación anterior de la misma ronda se probaron además las barras gráficas del
ecualizador, la importación desde el selector y el japonés.

No se ejecutaron los workflows de GitHub, `scripts/package.ps1` completo ni el instalador.

## Mediciones

Ryzen 7 5800X, 16 GB, Radeon RX 6800 XT (32.0.21045.29003), Windows 11 26200, ventana de
1800×1125 físicos al 125 %, copias del mismo perfil local de 1.503 pistas, volumen 2 %,
ambas ventanas con foco. CPU en porcentaje de un núcleo. GPU es el motor 3D del proceso.
RAM es working set / privada.

| Escenario | Iced DX12 (final) | GPUI |
| --- | --- | --- |
| CPU en reposo, 30 s | 0,10 % (0,10–0,47 % en tres compilaciones de la ronda) | 0,10 % |
| GPU en reposo | 0,07 % | 0,00 % |
| RAM en reposo | 150 / 154 MB | 91 / 119 MB |
| VRAM en reposo | 49 MB | 80 MB |
| Ventana visible | 1,0–1,8 s | 0,54–0,60 s |
| CPU acumulada tras unos 30 s de arranque | 4,9–7,9 s | 1,2 s |
| Letras en panel: CPU | 6,1 % | 18,1 % |
| Letras en panel: GPU media (máx.) | 2,24 % (5,31 %) | 3,40 % (7,55 %) |
| Pantalla completa: CPU | 5,7 % | 9,0 % |
| Pantalla completa: GPU media (máx.) | 1,99 % (5,17 %) | 4,63 % (8,40 %) |
| RAM en pantalla completa | 188 / 222 MB | 111 / 173 MB |
| Scroll: cambios de imagen por segundo (3 pases) | 200 (tope de la sonda) | 137–142 |
| Scroll: intervalo p95 | 6,0–6,3 ms | 11,3–14,7 ms |
| Scroll: intervalo máximo | 7,0–10,3 ms | 17,0–19,7 ms |
| Scroll: espera del hilo de interfaz, mediana / p95 | 4,7 / 5,0 ms | 1,2–1,3 / 5,0–5,1 ms |
| Scroll: CPU | 40–42 % | 68–69 % |
| Scroll: GPU | 9,5–9,9 % | 6,8–7,1 % |

Iced gana en CPU durante la animación y el scroll, en GPU durante la animación y en fluidez.
Pierde en RAM, arranque y GPU durante el scroll. En reposo iguala o queda algo por encima.

La fluidez se midió por cambios en una columna de pantalla, con rueda cada 15 ms y una sonda
que lee unas 200 veces por segundo. Es una aproximación, sin trazas de presentación.

Vulkan se descartó como predeterminado: unos 29 s de CPU del driver en cada arranque,
228 / 331 MB de RAM y 243 MB de VRAM en reposo. Con DX12 el driver aún gasta unos 4 s
de CPU al compilar shaders en cada arranque. Solo se ha medido en esta GPU.

## Error encontrado en el núcleo

Guardar etiquetas de MP3, FLAC o M4A fallaba con «acceso denegado» en Windows:
`NamedTempFile::persist` no puede sustituir el archivo que `exclusive_file` mantiene abierto.
Afecta también a GPUI y al código anterior a la migración (reproducido con FLAC; MP3 y M4A
usan el mismo código). `local::replace_held` lo corrige con `fs::rename`, con prueba de
regresión. Se comprobó en NTFS, no en FAT ni en red.

## Sin verificar

- Cuenta de TIDAL: inicio y cierre de sesión, inicio, búsqueda, streaming, DASH, favoritos,
  listas, radio, calidad, barra lateral y fichas con datos del catálogo.
- Last.fm (compilación sin credenciales) y Discord.
- WASAPI exclusivo, cambios de frecuencia y desconexión de dispositivo.
- Selector de carpetas y de portada, arrastrar archivos y reordenar filas, teclas
  multimedia (con el sondeo lento en reposo dependen del aviso de souvlaki).
- Ctrl y «+» con el teclado: la herramienta de pruebas no puede emitir esa tecla en la
  distribución española. Los botones de zoom, Ctrl+0, Ctrl y «-» y Ctrl + rueda sí funcionan.
- Inercia en menús y fichas. Rueda de touchpad.
- Replay con historial real, bienvenida, portugués, francés y alemán.
- Perfil real del usuario y perfiles de versiones anteriores a 1.6.0.
- Otros DPI y cambio de monitor, 4K, otras GPU, equipos sin DX12, sesiones largas.
- Movimiento reducido leído de Windows (no se cambió el ajuste del sistema).
- Apertura de GPUI desde la compilación con ambas interfaces (solo compila y pasa pruebas).

## Ronda de paridad y pruebas fuera de pantalla

- El buscador global ocupa el centro siempre que hay espacio. La lista de resultados toma la posición del mismo cálculo, incluso al estrechar la ventana. La cuadrícula admite una columna en ventanas estrechas. El menú de orden se ancla al botón y se sitúa encima cuando falta espacio debajo.
- Biblioteca recalcula su orden después de las cargas y cambios de colección. Las peticiones de otras pantallas ya no fuerzan ese trabajo. Las filas de Álbumes se virtualizan a ambos lados del álbum desplegado.
- Si una carga falla, Reintentar vuelve a pedir los datos de la misma página. El cierre de sesión descarta las vistas de catálogo, filtros rápidos y rangos dependientes de la sesión.
- El panel derecho y la barra lateral recuperan la apertura y el cierre con las duraciones de GPUI. Se recorta un widget de ancho fijo, de modo que las letras y portadas conservan su tamaño durante la transición. Los puntos de carga y las barras de reproducción piden redibujos locales a unos 30 Hz.
- El reproductor precarga imágenes de hasta dos pistas siguientes al preparar la cola. En la cola se atenúan las portadas de las pistas anteriores. El selector de idioma usa la paleta de Abyss y el preamplificador automático muestra cero sin signo y el control atenuado.
- La prueba con datos sintéticos de TIDAL comprueba el layout de 13 vistas con y sin espacio para centrar la búsqueda, el render de wgpu, la cola al reproducir desde un filtro y el reintento de Biblioteca. El cliente autenticado y el audio en una sesión real requieren una prueba de aceptación.

## Ronda de cristal, movimiento y espaciado

Tres subagentes trabajaron en copias separadas (`..\iced-work`), y un cuarto combinó los
cambios con fusión a tres bandas. Después se revisaron en pantalla junto a GPUI.

- **Cristal líquido** (`src/iced_ui/gpu/glass.rs`, `src/iced_ui/liquid_glass.rs`): sombreador
  wgpu con SDF de rectángulo redondeado, lente en el borde, dispersión de color, escarcha por
  niveles de desenfoque, brillo de borde que sigue al puntero y trazo de 1 px. Como Iced no
  permite leer lo ya pintado, refracta una imagen conocida: la portada de la página (o la del
  elemento destacado en Home, o la que suena), desenfocada en un hilo y en caché por URL; esa
  misma imagen tiñe la parte alta de la página. Ajustes en `Optics::PANEL`, `BAR` y `POPOVER`.
- **Movimiento** (`motion.rs`, `wash.rs`, `icons.rs::Glyph`, `panel_motion.rs`, `sheen.rs`):
  tiempos y curvas de `src/ui/interact.rs` y `motion.rs`; sin fotogramas cuando nada se mueve.
- **Espaciado**: anchos de columna, barra superior, barra de Biblioteca, menús, diálogos,
  búsqueda rápida, panel lateral, álbum desplegado, ajustes, Home y Replay reconstruido.
- Correcciones tras la revisión en pantalla: las etiquetas de sección de la barra lateral se
  cortaban (la pila tomaba el ancho de la regla), los raíles de los deslizadores desaparecían
  sobre el cristal y las casillas del resumen de Replay se recortaban.

## Ronda de cierre de paridad (24 de noviembre de 2026)

Revisión de cada diferencia pendiente contra el código de GPUI (`src/ui`). Varias ya estaban
portadas y el apartado anterior estaba desactualizado: brillo de la portada (`sheen.rs`),
corazón al doble clic, «pop» de los botones, lupa del filtro, borde y hover de los botones
superiores, separadores y estrellas al pasar en el álbum desplegado, y la cabecera de Replay.
La onda del botón de reproducir de GPUI nunca se activa (`haptics.ripple` no se asigna), así
que no hay nada que portar.

Portado en esta ronda:

- **Franja de color de los álbumes** (`page_wash`, modo `Wash` con luces): en la página de
  álbum y en el álbum desplegado de la colección, los tres colores principales de la portada
  derivan despacio por la franja superior (30 fps, sin fotogramas con movimiento reducido).
  Pesos al cuadrado para que cada color conserve su zona sin agrisarse, opacidad 0,80 en
  oscuro (antes 0,42 y atenuada al 80 %) y desvanecido más largo.
- **Gesto lateral del touchpad o rueda inclinable**: 150 px de desplazamiento horizontal van
  atrás o adelante, con la flecha en el borde que crece con el gesto y vuelve si no se completa.
- **Punta del álbum desplegado** que señala la tarjeta abierta.
- **Ajustes**: idioma con botones en vez de desplegable y primero en la lista, miniaturas del
  tema y del fondo del reproductor, salida de sonido con la lista desplegada en la tarjeta,
  Cuenta con perfil, datos personales, suscripción y seguridad, y «Acerca de» con la versión
  como título y el bloque «Sobre el proyecto».

Pruebas: 239 pasan, incluidas 4 de renderizado fuera de pantalla (nueva: la franja del álbum
brilla, muestra colores distintos a lo ancho y se desvanece).

## Diferencias conocidas con GPUI

- Texto: misma fuente, tamaños y corrección de DirectWrite, pero el rasterizado es de swash y no
  de DirectWrite; a lo sumo puede variar un píxel de borde. Valores fijos de Windows (gamma 1,8,
  contraste 1,0): si el usuario ajustó ClearType, GPUI los lee del sistema y Iced no.
- Cortes de línea (otro motor de texto).
- Fondo «Desenfoque» en Ajustes: la miniatura muestra los colores de la portada en degradado en
  vez de la portada desenfocada.
- Si el selector de archivos no puede abrirse no hay aviso: `rfd` no distingue el fallo de la
  cancelación.
- Con sesión de TIDAL, Iced añade flechas de atrás y adelante que GPUI no muestra (se mantienen).
- No portado por inalcanzable en GPUI: menú de aplicación y acceso web PKCE.
- Si wgpu no puede iniciar la ventana, aparece un cuadro de error y el diagnóstico queda en `startup.log` del perfil. Sigue pendiente un renderizador alternativo.
