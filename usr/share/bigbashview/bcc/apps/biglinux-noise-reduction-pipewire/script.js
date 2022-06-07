// $(function () {
//   var tab = $("li label");
//   tab.on("click", function (event) {
//     //event.preventDefault();
//     //tab.removeClass("active");
//     //$(this).addClass("active");
//     tab_content = $(this).attr("id");
//     //alert(tab_content);
//     $('div[id$="tab-content"]').removeClass("active");
//     $(tab_content).addClass("active");
//   });
// });


// $(function () {
//  $(".menu-link").click(function () {
//   $(".menu-link").removeClass("is-active");
//   $(this).addClass("is-active");
//  });
// });
// 
// $(function () {
//  $(".main-header-link").click(function () {
//   $(".main-header-link").removeClass("is-active");
//   $(this).addClass("is-active");
//  });
// });
// 
// const dropdowns = document.querySelectorAll(".dropdown");
// dropdowns.forEach((dropdown) => {
//  dropdown.addEventListener("click", (e) => {
//   e.stopPropagation();
//   dropdowns.forEach((c) => c.classList.remove("is-active"));
//   dropdown.classList.add("is-active");
//  });
// });
// 
// $(".search-bar input")
//  .focus(function () {
//   $(".header").addClass("wide");
//  })
//  .blur(function () {
//   $(".header").removeClass("wide");
//  });
// 
// $(document).click(function (e) {
//  var container = $(".status-button");
//  var dd = $(".dropdown");
//  if (!container.is(e.target) && container.has(e.target).length === 0) {
//   dd.removeClass("is-active");
//  }
// });
// 
// $(function () {
//  $(".dropdown").on("click", function (e) {
//   $(".content-wrapper").addClass("overlay");
//   e.stopPropagation();
//  });
//  $(document).on("click", function (e) {
//   if ($(e.target).is(".dropdown") === false) {
//    $(".content-wrapper").removeClass("overlay");
//   }
//  });
// });
// 
$(function () {
 $(".status-button:not(.open)").on("click", function (e) {
  $(".overlay-app").addClass("is-active");
 });
 $(".pop-up .close").click(function () {
  $(".overlay-app").removeClass("is-active");
 });
});

$(".status-button:not(.open)").click(function () {
 $(".pop-up").addClass("visible");
});

$(".pop-up .close").click(function () {
 $(".pop-up").removeClass("visible");
});


const toggleButton = document.querySelector(".dark-light");

toggleButton.addEventListener("click", () => {
  document.body.classList.toggle("light-mode");
  _run('/usr/share/bigbashview/bcc/shell/setbgcolor.sh "' + document.body.classList.contains('light-mode') + '"');
});



//mic 
class AudioVisualizer {
  constructor( audioContext, processFrame, processError ) {
    this.audioContext = audioContext;
    this.processFrame = processFrame;
    this.connectStream = this.connectStream.bind( this );
    navigator.mediaDevices.getUserMedia( { audio: true, video: false } )
      .then( this.connectStream )
      .catch( ( error ) => {
        if ( processError ) {
          processError( error );
        }
      } );
  }

  connectStream( stream ) {
    this.analyser = this.audioContext.createAnalyser();
    const source = this.audioContext.createMediaStreamSource( stream );
    source.connect( this.analyser );
    this.analyser.smoothingTimeConstant = 0.5;
    this.analyser.fftSize = 32;

    this.initRenderLoop( this.analyser );
  }

  initRenderLoop() {
    const frequencyData = new Uint8Array( this.analyser.frequencyBinCount );
    const processFrame = this.processFrame || ( () => {} );

    const renderFrame = () => {
      this.analyser.getByteFrequencyData( frequencyData );
      processFrame( frequencyData );

      requestAnimationFrame( renderFrame );
    };
    requestAnimationFrame( renderFrame );
  }
}

const visualMainElement = document.querySelector( 'main' );
const visualValueCount = 16;
let visualElements;
const createDOMElements = () => {
  let i;
  for ( i = 0; i < visualValueCount; ++i ) {
    const elm = document.createElement( 'div' );
    visualMainElement.appendChild( elm );
  }

  visualElements = document.querySelectorAll( 'main div' );
};
createDOMElements();

const init = () => {
  // Creating initial DOM elements
  const audioContext = new AudioContext();
  const initDOM = () => {
    visualMainElement.innerHTML = '';
    createDOMElements();
  };
  initDOM();

  // Swapping values around for a better visual effect
  const dataMap = { 0: 15, 1: 10, 2: 8, 3: 9, 4: 6, 5: 5, 6: 2, 7: 1, 8: 0, 9: 4, 10: 3, 11: 7, 12: 11, 13: 12, 14: 13, 15: 14 };
  const processFrame = ( data ) => {
    const values = Object.values( data );
    let i;
    for ( i = 0; i < visualValueCount; ++i ) {
      const value = values[ dataMap[ i ] ] / 255;
      const elmStyles = visualElements[ i ].style;
      elmStyles.transform = `scaleY( ${ value } )`;
      elmStyles.opacity = Math.max( .25, value );
    }
  };

  const processError = () => {
    visualMainElement.classList.add( 'error' );
    visualMainElement.innerText = 'Permita o acesso ao seu microfone para ver esta demonstração.';
  }

  const a = new AudioVisualizer( audioContext, processFrame, processError );
};
